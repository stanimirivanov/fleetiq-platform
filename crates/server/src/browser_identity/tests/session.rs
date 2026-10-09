//! Current identity binding and CSRF-protected session revocation.

use super::*;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn sessions_recheck_expiry_activation_and_original_actor_mapping(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    seed_session(
        &pool,
        UnixTimeMillis::new(now().expect("test clock").get() - 1),
    )
    .await?;
    let authentication = BrowserRequestAuthenticator::new(pool.clone(), &config());
    let headers = request("/", Some(&secret_cookie(SESSION_COOKIE, SESSION_SECRET)))
        .into_parts()
        .0
        .headers;
    assert!(authentication.authenticate(&headers).await.is_err());
    PostgresBrowserIdentityRepository::new(pool.clone())
        .delete_session(&digest(SESSION_SECRET))
        .await?;
    seed_session(
        &pool,
        UnixTimeMillis::new(now().expect("test clock").get() + 60_000),
    )
    .await?;
    assert_eq!(
        authentication
            .authenticate(&headers)
            .await
            .map_err(|_| "authentication failed")?
            .actor_id()
            .as_str(),
        "actor-a"
    );
    for (issuer, client_id, authority) in [
        ("https://different.example", "client", "workforce"),
        ("https://identity.example", "different-client", "workforce"),
        ("https://identity.example", "client", "different-authority"),
    ] {
        let changed =
            BrowserIdentityConfig::from_values(ORIGIN, issuer, client_id, "secret", authority)?;
        assert!(
            BrowserRequestAuthenticator::new(pool.clone(), &changed)
                .authenticate(&headers)
                .await
                .is_err()
        );
    }
    let rotated_secret = BrowserIdentityConfig::from_values(
        ORIGIN,
        "https://identity.example",
        "client",
        "rotated-secret",
        "workforce",
    )?;
    assert!(
        BrowserRequestAuthenticator::new(pool.clone(), &rotated_secret)
            .authenticate(&headers)
            .await
            .is_ok()
    );
    sqlx::query("UPDATE identity.actors SET is_active = false WHERE actor_id = 'actor-a'")
        .execute(&pool)
        .await?;
    assert!(authentication.authenticate(&headers).await.is_err());
    sqlx::query("UPDATE identity.actors SET is_active = true WHERE actor_id = 'actor-a'")
        .execute(&pool)
        .await?;
    sqlx::query("UPDATE identity.external_actor_identities SET actor_id = 'actor-b' WHERE authority_id = 'workforce' AND subject = 'operator-1'").execute(&pool).await?;
    assert!(authentication.authenticate(&headers).await.is_err());
    sqlx::query("UPDATE identity.external_actor_identities SET actor_id = 'actor-a' WHERE authority_id = 'workforce' AND subject = 'operator-1'").execute(&pool).await?;
    let mut ambiguous = headers.clone();
    ambiguous.insert(header::AUTHORIZATION, "Bearer workload".parse()?);
    assert!(authentication.authenticate(&ambiguous).await.is_err());
    sqlx::query("DELETE FROM identity.external_actor_identities WHERE authority_id = 'workforce' AND subject = 'operator-1'").execute(&pool).await?;
    assert!(authentication.authenticate(&headers).await.is_err());
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn logout_requires_exact_origin_and_custom_csrf_header_then_revokes(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    seed_session(
        &pool,
        UnixTimeMillis::new(now().expect("test clock").get() + 60_000),
    )
    .await?;
    let app = router(pool.clone(), Arc::new(AtomicUsize::new(0)));
    let unsupported = app
        .clone()
        .oneshot(request("/api/v1/auth/logout", None))
        .await?;
    assert_eq!(unsupported.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(unsupported.headers()[header::ALLOW], "POST");
    assert_eq!(unsupported.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        unsupported.headers()[header::REFERRER_POLICY],
        "no-referrer"
    );
    for (origin, csrf) in [
        (None, None),
        (Some("https://attacker.example"), Some("1")),
        (Some(ORIGIN), None),
        (Some(ORIGIN), Some("wrong")),
    ] {
        let mut request = Request::post("/api/v1/auth/logout").header(
            header::COOKIE,
            secret_cookie(SESSION_COOKIE, SESSION_SECRET),
        );
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        if let Some(csrf) = csrf {
            request = request.header("x-fleetiq-csrf", csrf);
        }
        let response = app.clone().oneshot(request.body(Body::empty())?).await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert!(
        PostgresBrowserIdentityRepository::new(pool.clone())
            .find_session(&digest(SESSION_SECRET), now().expect("test clock"))
            .await?
            .is_some()
    );
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/logout")
                .header(header::ORIGIN, ORIGIN)
                .header("x-fleetiq-csrf", "1")
                .header(
                    header::COOKIE,
                    secret_cookie(SESSION_COOKIE, SESSION_SECRET),
                )
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .all(|value| value.to_str().unwrap().contains("Max-Age=0"))
    );
    let repeated = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/logout")
                .header(header::ORIGIN, ORIGIN)
                .header("x-fleetiq-csrf", "1")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(repeated.status(), StatusCode::NO_CONTENT);
    let after = app
        .oneshot(request(
            "/api/v1/auth/session",
            Some(&secret_cookie(SESSION_COOKIE, SESSION_SECRET)),
        ))
        .await?;
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
    Ok(())
}
