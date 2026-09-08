//! Billing query contracts reject malformed rows, cross-scope continuations, and lossy amounts.
use crate::billing_azure;
use serde_json::json;
#[test]
#[ignore = "requires an explicitly captured HEALTHCHECK_AZURE_BILLING_RESPONSE file"]
fn captured_azure_response_projects_all_rows() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;
    let path = std::env::var("HEALTHCHECK_AZURE_BILLING_RESPONSE")?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("capture exceeds response limit".into());
    }
    let page = billing_azure::decode(&bytes, "validation")?;
    assert!(!page.rows.is_empty());
    Ok(())
}
#[test]
fn azure_fractional_usage_preserves_submicro_currency_charges()
-> Result<(), Box<dyn std::error::Error>> {
    let value = r#"{"properties":{"columns":[{"name":"Cost"},{"name":"UsageDate"},{"name":"ServiceName"},{"name":"ResourceId"},{"name":"Currency"}],"rows":[[7.32578337192535e-07,20260901,"Storage",null,"USD"]],"nextLink":null}}"#;
    let page = billing_azure::decode(value.as_bytes(), "example")?;
    assert_eq!(
        String::from(page.rows[0].billed.clone()),
        "0.000000732578337192535"
    );
    Ok(())
}
#[test]
fn azure_query_keeps_decimal_lexemes_and_resource_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let value = r#"{"properties":{"columns":[{"name":"Cost"},{"name":"UsageDate"},{"name":"ServiceName"},{"name":"ResourceId"},{"name":"Currency"}],"rows":[[0.16677720329728665,20260901,"Compute","/subscriptions/example/resourceGroups/rg/providers/Microsoft.Compute/virtualMachines/vm","USD"]],"nextLink":null}}"#;
    let page = billing_azure::decode(value.as_bytes(), "example")?;
    assert_eq!(
        String::from(page.rows[0].billed.clone()),
        "0.16677720329728665"
    );
    assert!(page.rows[0].invoice_month.is_none());
    assert!(
        page.rows[0]
            .resource
            .as_ref()
            .is_some_and(|r| r.contains("microsoft.compute"))
    );
    assert!(page.next.is_none());
    Ok(())
}
#[test]
fn azure_pagination_cannot_change_origin_or_subscription() -> Result<(), Box<dyn std::error::Error>>
{
    let root = "https://management.azure.com/subscriptions/example/providers/Microsoft.CostManagement/query?api-version=2026-06-01".parse()?;
    for bad in [
        "https://attacker.example/subscriptions/example/providers/Microsoft.CostManagement/query",
        "https://management.azure.com/subscriptions/other/providers/Microsoft.CostManagement/query",
        "https://management.azure.com/subscriptions/example/providers/Microsoft.CostManagement/query?redirect=https://attacker.example",
    ] {
        assert!(billing_azure::continuation(&root, bad).is_err());
    }
    assert!(billing_azure::continuation(&root, "https://management.azure.com/subscriptions/example/providers/Microsoft.CostManagement/Query?api-version=2026-06-01&$skiptoken=next").is_ok());
    assert!(
        billing_azure::decode(
            &serde_json::to_vec(&json!({"properties":{"columns":[],"rows":[[]]}}))?,
            "example"
        )
        .is_err()
    );
    Ok(())
}
