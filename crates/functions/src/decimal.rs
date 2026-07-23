use ir::Value;

pub(crate) fn product(values: &[Value]) -> Option<f64> {
    let mut values = values.iter();
    let first = ExactDecimal::from_value(values.next()?)?;
    let product = values.try_fold(first, |product, value| {
        product.multiply(ExactDecimal::from_value(value)?)
    })?;
    let value = format!("{}e{}", product.coefficient, product.exponent)
        .parse::<f64>()
        .ok()?;
    value.is_finite().then_some(value)
}

#[derive(Clone, Copy)]
struct ExactDecimal {
    coefficient: i128,
    exponent: i32,
}

impl ExactDecimal {
    fn from_value(value: &Value) -> Option<Self> {
        let lexical = match value {
            Value::Int(value) => value.to_string(),
            Value::Float(value) if value == &0.0 && value.is_sign_negative() => return None,
            Value::Float(value) if value.is_finite() => value.to_string(),
            Value::String(value) => {
                let value = value.trim();
                if let Ok(value) = value.parse::<i64>() {
                    value.to_string()
                } else {
                    let value = value.parse::<f64>().ok()?;
                    if !value.is_finite() || value == 0.0 && value.is_sign_negative() {
                        return None;
                    }
                    value.to_string()
                }
            }
            Value::Null
            | Value::JsonNull(_)
            | Value::XmlNil(_)
            | Value::Bool(_)
            | Value::Float(_) => return None,
        };
        Self::parse(&lexical)
    }

    fn parse(lexical: &str) -> Option<Self> {
        let (mantissa, scientific_exponent) = match lexical.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
            None => (lexical, 0),
        };
        let (negative, mantissa) = mantissa
            .strip_prefix('-')
            .map_or((false, mantissa), |mantissa| (true, mantissa));
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if whole.is_empty()
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let mut digits = String::with_capacity(whole.len() + fraction.len());
        digits.push_str(whole);
        digits.push_str(fraction);
        let mut coefficient = digits.parse::<i128>().ok()?;
        if negative {
            coefficient = coefficient.checked_neg()?;
        }
        let fraction_digits = i32::try_from(fraction.len()).ok()?;
        let exponent = scientific_exponent.checked_sub(fraction_digits)?;
        Some(
            Self {
                coefficient,
                exponent,
            }
            .normalized(),
        )
    }

    fn multiply(self, other: Self) -> Option<Self> {
        Some(
            Self {
                coefficient: self.coefficient.checked_mul(other.coefficient)?,
                exponent: self.exponent.checked_add(other.exponent)?,
            }
            .normalized(),
        )
    }

    fn normalized(mut self) -> Self {
        while self.coefficient != 0 && self.coefficient % 10 == 0 {
            self.coefficient /= 10;
            self.exponent += 1;
        }
        self
    }
}
