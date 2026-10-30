//! Explicit wire projection: exact epoch times and decimals remain strings.
use fleetiq_postgres::{CalculatorEvidenceRow, CalculatorOutputRow};
use serde_json::{Value, json};
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, ()> {
    v[key].as_str().ok_or(())
}
fn millis(v: &Value, key: &str) -> Result<String, ()> {
    v[key].as_i64().map(|n| n.to_string()).ok_or(())
}
pub(super) fn summary(r: &CalculatorEvidenceRow) -> Result<Value, ()> {
    let k = &r.replay;
    let implementation =
        fleetiq_analytics::CalculatorImplementation::parse(&r.implementation).map_err(|_| ())?;
    if !matches!(
        r.outcome.as_str(),
        "complete" | "insufficient" | "incompatible_input" | "invalid_output" | "resource_limit"
    ) || !(1..=1024).contains(&r.minimum_samples)
        || !(0..=1024).contains(&r.input_count)
        || !(0..=1024).contains(&r.output_count)
    {
        return Err(());
    }
    Ok(
        json!({"evaluation_id":r.id,"scope_asset_id":text(k,"scope")?,"calculator":{"id":text(k,"calculator")?,"version":text(k,"version")?,"implementation":implementation.as_str()},"configuration_id":text(k,"configuration")?,"input_property":{"id":r.input_property_id,"version":u32::try_from(r.input_property_version).map_err(|_|())?},"window":{"start_at_ms":millis(k,"start")?,"end_at_ms":millis(k,"end")?,"finalize_at_ms":millis(k,"finalize")?,"known_at_ms":millis(k,"known")?},"evaluated_at_ms":r.evaluated_at_ms.to_string(),"outcome":r.outcome,"minimum_samples":r.minimum_samples,"input_count":r.input_count,"output_count":r.output_count}),
    )
}
pub(super) fn exact(r: &CalculatorEvidenceRow) -> Result<Value, ()> {
    let inputs = r.replay["inputs"].as_array().ok_or(())?;
    if inputs.len() > 1024 || inputs.len() != usize::try_from(r.input_count).map_err(|_| ())? {
        return Err(());
    }
    let inputs=inputs.iter().map(|i|Ok(json!({"evidence_id":i[0].as_str().ok_or(())?,"event_at_ms":i[1].as_i64().ok_or(())?.to_string()}))).collect::<Result<Vec<Value>,()>>()?;
    Ok(json!({"evaluation":summary(r)?,"inputs":inputs}))
}
pub(super) fn output(r: &CalculatorOutputRow, e: &CalculatorEvidenceRow) -> Result<Value, ()> {
    let p = &r.output;
    let value = text(p, "value")?;
    if !(0..1024).contains(&r.index) || text(p, "kind")? != "decimal" || value.len() > 4096 {
        return Err(());
    }
    let version = u32::try_from(p["version"].as_u64().ok_or(())?).map_err(|_| ())?;
    let unit = match &p["unit"] {
        Value::Null => None,
        Value::String(v) => Some(v),
        _ => return Err(()),
    };
    Ok(
        json!({"evaluation_id":e.id,"calculator_id":text(&e.replay,"calculator")?,"calculator_version":text(&e.replay,"version")?,"output_index":r.index,"asset_id":text(p,"asset")?,"property_id":text(p,"property")?,"property_version":version,"kind":"decimal","value":value,"unit":unit}),
    )
}
