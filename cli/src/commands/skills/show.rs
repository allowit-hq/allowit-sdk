//! `allowit skills show ID`.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
};
use serde_json::Value;

pub(crate) const EXAMPLES: &str = "\
Examples:
  allowit skills show SERVICE_ID
  allowit skills show SERVICE_ID --json";

/// Show one catalog service and its endpoints
#[derive(clap::Args, Debug)]
pub(crate) struct Args {
    /// The service ID
    pub id: String,
}

/// Finds the entry whose `id` or `serviceId` is exactly `id`.
pub(crate) fn find(value: &Value, id: &str) -> Option<Value> {
    match value {
        Value::Object(map) => {
            if ["id", "serviceId"]
                .iter()
                .any(|k| map.get(*k).and_then(Value::as_str) == Some(id))
            {
                return Some(value.clone());
            }
            map.values().find_map(|v| find(v, id))
        }
        Value::Array(items) => items.iter().find_map(|v| find(v, id)),
        _ => None,
    }
}

pub(crate) fn run(args: Args, cx: &mut Ctx) -> Result<i32> {
    let value = super::read(Some(&args.id))?;
    let service = find(&value, &args.id)
        .ok_or_else(|| Error::config(format!("the catalog has no service {}", args.id)))?;
    cx.out.json_value(&service);
    Ok(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn show_picks_the_exact_service_not_the_search_results() {
        let catalog = serde_json::json!({"services":[
            {"id":"weather-pro","name":"Weather Pro"},
            {"id":"weather","name":"Weather"}
        ]});
        assert_eq!(super::find(&catalog, "weather").unwrap()["name"], "Weather");
        assert!(super::find(&catalog, "weath").is_none());
    }
}
