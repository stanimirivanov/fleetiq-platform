//! Browser-bound single-use login and callback behavior.

use super::*;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn login_binds_callback_to_browser_and_rotates_the_session(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    seed_session(
        &pool,
        UnixTimeMillis::new(now().expect("test clock").get() + 60_000),
    )
    .await?;
    let exchanges = Arc::new(AtomicUsize::new(0));
    let app = router(pool.clone(), exchanges.clone());
    let login = app
        .clone()
        .oneshot(request("/api/v1/auth/login", None))
        .await?;
    assert_eq!(login.status(), StatusCode::SEE_OTHER);
    assert_eq!(login.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(login.headers()[header::REFERRER_POLICY], "no-referrer");
    let login_cookie = login.headers()[header::SET_COOKIE].to_str()?;
    for attribute in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
        assert!(login_cookie.contains(attribute));
    }
    assert!(!login_cookie.contains("Domain="));
    let login_cookie = login_cookie.split(';').next().unwrap().to_owned();
    let callback_path =
        "/api/v1/auth/callback?code=valid-code&state=test-state&return_to=https://attacker.example";
    for path in [
        "/api/v1/auth/callback?code=valid-code&code=second-code&state=test-state",
        "/api/v1/auth/callback?code=valid-code&state=test-state&state=second-state",
    ] {
        let duplicate = app
            .clone()
            .oneshot(request(path, Some(&login_cookie)))
            .await?;
        assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(exchanges.load(Ordering::SeqCst), 0);
    let wrong_browser = app
        .clone()
        .oneshot(request(
            callback_path,
            Some(&secret_cookie(LOGIN_COOKIE, LOGIN_SECRET)),
        ))
        .await?;
    assert_eq!(wrong_browser.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(exchanges.load(Ordering::SeqCst), 0);
    let cookies = format!(
        "{login_cookie}; {}",
        secret_cookie(SESSION_COOKIE, SESSION_SECRET)
    );
    let callback = app
        .clone()
        .oneshot(request(callback_path, Some(&cookies)))
        .await?;
    assert_eq!(callback.status(), StatusCode::SEE_OTHER);
    assert_eq!(callback.headers()[header::LOCATION], "/");
    assert_eq!(exchanges.load(Ordering::SeqCst), 1);
    let session_cookie = callback
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|value| {
            let text = value.to_str().ok()?;
            text.starts_with(SESSION_COOKIE)
                .then(|| text.split(';').next().unwrap().to_owned())
        })
        .expect("new session cookie");
    assert_ne!(
        session_cookie,
        secret_cookie(SESSION_COOKIE, SESSION_SECRET)
    );
    assert!(
        PostgresBrowserIdentityRepository::new(pool.clone())
            .find_session(&digest(SESSION_SECRET), now().expect("test clock"))
            .await?
            .is_none()
    );
    let current = app
        .clone()
        .oneshot(request("/api/v1/auth/session", Some(&session_cookie)))
        .await?;
    assert_eq!(current.status(), StatusCode::OK);
    let payload: serde_json::Value =
        serde_json::from_slice(&to_bytes(current.into_body(), 4_096).await?)?;
    let contract: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/http/dist/openapi.json",
    )))?;
    let validator =
        jsonschema::validator_for(&contract["components"]["schemas"]["OperatorSession"])?;
    assert!(
        validator.is_valid(&payload),
        "actual session response follows the published schema"
    );
    assert_eq!(payload["actor_id"], "actor-a");
    assert!(payload.get("token").is_none());
    assert!(
        payload["expires_at_ms"].as_i64().unwrap() <= now().expect("test clock").get() + 60_000
    );
    let replay = app.oneshot(request(callback_path, Some(&cookies))).await?;
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(exchanges.load(Ordering::SeqCst), 1);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn unregistered_identity_cannot_self_provision_and_failed_exchange_is_one_use(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let exchanges = Arc::new(AtomicUsize::new(0));
    let app = router(pool.clone(), exchanges.clone());
    let login = app
        .clone()
        .oneshot(request("/api/v1/auth/login", None))
        .await?;
    let login_cookie = login.headers()[header::SET_COOKIE]
        .to_str()?
        .split(';')
        .next()
        .unwrap();
    let response = app
        .clone()
        .oneshot(request(
            "/api/v1/auth/callback?code=invalid-code&state=test-state",
            Some(login_cookie),
        ))
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let retry = app
        .clone()
        .oneshot(request(
            "/api/v1/auth/callback?code=valid-code&state=test-state",
            Some(login_cookie),
        ))
        .await?;
    assert_eq!(retry.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(exchanges.load(Ordering::SeqCst), 1);
    let login = app
        .clone()
        .oneshot(request("/api/v1/auth/login", None))
        .await?;
    let login_cookie = login.headers()[header::SET_COOKIE]
        .to_str()?
        .split(';')
        .next()
        .unwrap();
    let unregistered = app
        .oneshot(request(
            "/api/v1/auth/callback?code=valid-code&state=test-state",
            Some(login_cookie),
        ))
        .await?;
    assert_eq!(unregistered.status(), StatusCode::UNAUTHORIZED);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM identity.actors")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn expired_login_attempts_never_exchange_codes_and_errors_are_safe(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    PostgresBrowserIdentityRepository::new(pool.clone())
        .insert_login_attempt(&fleetiq_postgres::BrowserLoginAttempt {
            state_digest: digest("expired-state"),
            provider_digest: provider_digest(&config()),
            browser_digest: digest(LOGIN_SECRET),
            pkce_verifier: PKCE_VERIFIER.to_owned(),
            nonce: NONCE.to_owned(),
            expires_at: UnixTimeMillis::new(now().expect("test clock").get() - 1),
        })
        .await?;
    let exchanges = Arc::new(AtomicUsize::new(0));
    let app = router(pool, exchanges.clone());
    let cookie = secret_cookie(LOGIN_COOKIE, LOGIN_SECRET);
    let response = app
        .clone()
        .oneshot(request(
            "/api/v1/auth/callback?code=private-code&state=expired-state",
            Some(&cookie),
        ))
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
    let body = String::from_utf8(to_bytes(response.into_body(), 4_096).await?.to_vec())?;
    assert!(!body.contains("private-code"));
    assert!(!body.contains("expired-state"));
    assert_eq!(exchanges.load(Ordering::SeqCst), 0);
    let method = app
        .oneshot(Request::post("/api/v1/auth/session").body(Body::empty())?)
        .await?;
    assert_eq!(method.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(method.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(method.headers()[header::REFERRER_POLICY], "no-referrer");
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn provider_reconfiguration_rejects_pending_callbacks_before_code_exchange(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_identity(&pool).await?;
    let exchanges = Arc::new(AtomicUsize::new(0));
    let original = router(pool.clone(), exchanges.clone());
    let login = original
        .clone()
        .oneshot(request("/api/v1/auth/login", None))
        .await?;
    let browser_cookie = login.headers()[header::SET_COOKIE]
        .to_str()?
        .split(';')
        .next()
        .unwrap();
    let changed_config = BrowserIdentityConfig::from_values(
        ORIGIN,
        "https://identity.example",
        "client",
        "secret",
        "different-authority",
    )?;
    let changed = postgres_browser_router(
        pool,
        changed_config,
        TestProvider {
            exchanges: exchanges.clone(),
        },
    );
    let path = "/api/v1/auth/callback?code=valid-code&state=test-state";
    let response = changed.oneshot(request(path, Some(browser_cookie))).await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(exchanges.load(Ordering::SeqCst), 0);
    // The failed claim is consumed; reverting configuration cannot replay it.
    let replay = original
        .oneshot(request(path, Some(browser_cookie)))
        .await?;
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(exchanges.load(Ordering::SeqCst), 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn logout_cancels_pending_browser_login_before_code_exchange(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let exchanges = Arc::new(AtomicUsize::new(0));
    let app = router(pool, exchanges.clone());
    let login = app
        .clone()
        .oneshot(request("/api/v1/auth/login", None))
        .await?;
    let browser_cookie = login.headers()[header::SET_COOKIE]
        .to_str()?
        .split(';')
        .next()
        .unwrap();
    let logout = app
        .clone()
        .oneshot(
            Request::post("/api/v1/auth/logout")
                .header(header::ORIGIN, ORIGIN)
                .header("x-fleetiq-csrf", "1")
                .header(header::COOKIE, browser_cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    let callback = app
        .oneshot(request(
            "/api/v1/auth/callback?code=valid-code&state=test-state",
            Some(browser_cookie),
        ))
        .await?;
    assert_eq!(callback.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(exchanges.load(Ordering::SeqCst), 0);
    Ok(())
}
