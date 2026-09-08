//! Billing query contracts reject malformed rows, cross-scope continuations, and lossy amounts.
use crate::billing_azure;
use serde_json::json;
#[test]
fn azure_query_keeps_decimal_lexemes_and_resource_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let value = serde_json::from_str(
        r#"{"properties":{"columns":[{"name":"Cost"},{"name":"UsageDate"},{"name":"ServiceName"},{"name":"ResourceId"},{"name":"Currency"}],"rows":[[0.16677720329728665,20260901,"Compute","/subscriptions/example/resourceGroups/rg/providers/Microsoft.Compute/virtualMachines/vm","USD"]],"nextLink":null}}"#,
    )?;
    let page = billing_azure::decode(value, "example")?;
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
        billing_azure::decode(json!({"properties":{"columns":[],"rows":[[]]}}), "example").is_err()
    );
    Ok(())
}
