use std::error::Error;
use std::fmt;

use codegen_runtime::{Instance, Value, field, group, repeated, scalar};
use ferrule_generated_mapping::execute;

fn main() -> Result<(), Box<dyn Error>> {
    let output = execute(&source())?;
    let invoices = invoices(&output)?;

    let expected = [
        Invoice {
            sequence: 1,
            customer: "Ada",
            display: "ADA / 30.00 EUR",
            amount: 30.0,
        },
        Invoice {
            sequence: 2,
            customer: "Ada",
            display: "ADA / 12.25 USD",
            amount: 12.25,
        },
        Invoice {
            sequence: 3,
            customer: "Lin",
            display: "LIN / 19.50 USD",
            amount: 19.5,
        },
    ];
    assert_eq!(invoices, expected);

    println!("Generated {} invoices:", invoices.len());
    for invoice in invoices {
        println!(
            "  {}. {}: {} (amount {:.2})",
            invoice.sequence, invoice.customer, invoice.display, invoice.amount
        );
    }

    Ok(())
}

fn source() -> Instance {
    group([field(
        "Orders",
        repeated([
            order("Lin", 19.5, "USD"),
            order("Ada", 12.25, "USD"),
            order("Ignore", 0.0, "USD"),
            order("Ada", 30.0, "EUR"),
        ]),
    )])
}

fn order(customer: &str, amount: f64, currency: &str) -> Instance {
    group([
        field("Customer", scalar(Value::String(customer.into()))),
        field("Amount", scalar(Value::Float(amount))),
        field("Currency", scalar(Value::String(currency.into()))),
    ])
}

#[derive(Debug, PartialEq)]
struct Invoice<'a> {
    sequence: i64,
    customer: &'a str,
    display: &'a str,
    amount: f64,
}

fn invoices(output: &Instance) -> Result<Vec<Invoice<'_>>, OutputShapeError> {
    let items = output
        .field("Invoices")
        .and_then(Instance::as_repeated)
        .ok_or_else(|| OutputShapeError::new("Invoices must be a repeated field"))?;

    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            Ok(Invoice {
                sequence: integer_field(item, "Sequence", index)?,
                customer: string_field(item, "Customer", index)?,
                display: string_field(item, "Display", index)?,
                amount: float_field(item, "Amount", index)?,
            })
        })
        .collect()
}

fn integer_field(item: &Instance, name: &str, index: usize) -> Result<i64, OutputShapeError> {
    match scalar_field(item, name, index)? {
        Value::Int(value) => Ok(*value),
        value => Err(wrong_scalar_type(name, index, "int", value)),
    }
}

fn float_field(item: &Instance, name: &str, index: usize) -> Result<f64, OutputShapeError> {
    match scalar_field(item, name, index)? {
        Value::Float(value) => Ok(*value),
        value => Err(wrong_scalar_type(name, index, "float", value)),
    }
}

fn string_field<'a>(
    item: &'a Instance,
    name: &str,
    index: usize,
) -> Result<&'a str, OutputShapeError> {
    match scalar_field(item, name, index)? {
        Value::String(value) => Ok(value),
        value => Err(wrong_scalar_type(name, index, "string", value)),
    }
}

fn scalar_field<'a>(
    item: &'a Instance,
    name: &str,
    index: usize,
) -> Result<&'a Value, OutputShapeError> {
    item.field(name)
        .and_then(Instance::as_scalar)
        .ok_or_else(|| {
            OutputShapeError::new(format!("Invoices[{index}].{name} must be a scalar field"))
        })
}

fn wrong_scalar_type(name: &str, index: usize, expected: &str, actual: &Value) -> OutputShapeError {
    OutputShapeError::new(format!(
        "Invoices[{index}].{name} must be {expected}, got {}",
        actual.type_name()
    ))
}

#[derive(Debug)]
struct OutputShapeError(String);

impl OutputShapeError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for OutputShapeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for OutputShapeError {}
