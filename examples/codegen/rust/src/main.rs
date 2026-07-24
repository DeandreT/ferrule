use std::error::Error;

use ferrule_generated_mapping::execute_json;

fn main() -> Result<(), Box<dyn Error>> {
    let output = execute_json(include_str!("../../input.json"))?;
    let actual: serde_json::Value = serde_json::from_str(&output)?;
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../../expected-output.json"))?;
    assert_eq!(actual, expected);

    let invoices = actual
        .get("Invoices")
        .and_then(serde_json::Value::as_array)
        .ok_or("generated output does not contain an Invoices array")?;
    println!("Generated {} invoices:", invoices.len());
    for invoice in invoices {
        let sequence = field(invoice, "Sequence")?
            .as_i64()
            .ok_or("generated invoice Sequence is not an integer")?;
        let customer = field(invoice, "Customer")?
            .as_str()
            .ok_or("generated invoice Customer is not a string")?;
        let display = field(invoice, "Display")?
            .as_str()
            .ok_or("generated invoice Display is not a string")?;
        let amount = field(invoice, "Amount")?
            .as_f64()
            .ok_or("generated invoice Amount is not a number")?;
        println!("  {sequence}. {customer}: {display} (amount {amount:.2})");
    }

    Ok(())
}

fn field<'a>(
    invoice: &'a serde_json::Value,
    name: &str,
) -> Result<&'a serde_json::Value, Box<dyn Error>> {
    invoice
        .get(name)
        .ok_or_else(|| format!("generated invoice does not contain {name}").into())
}
