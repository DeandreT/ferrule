use std::error::Error;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WsdlMessageRole {
    Request,
    Response,
    Fault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WsdlMessageOptions {
    contract_file: String,
    service: String,
    port: String,
    operation: String,
    role: WsdlMessageRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fault_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsdlMessageOptionsError {
    EmptyContractFile,
    EmptyService,
    EmptyPort,
    EmptyOperation,
    MissingFaultName,
    UnexpectedFaultName,
}

impl fmt::Display for WsdlMessageOptionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyContractFile => "WSDL contract file cannot be empty",
            Self::EmptyService => "WSDL service cannot be empty",
            Self::EmptyPort => "WSDL port cannot be empty",
            Self::EmptyOperation => "WSDL operation cannot be empty",
            Self::MissingFaultName => "a WSDL fault message requires a fault name",
            Self::UnexpectedFaultName => "only a WSDL fault message can carry a fault name",
        };
        formatter.write_str(message)
    }
}

impl Error for WsdlMessageOptionsError {}

impl WsdlMessageOptions {
    pub fn new(
        contract_file: impl Into<String>,
        service: impl Into<String>,
        port: impl Into<String>,
        operation: impl Into<String>,
        role: WsdlMessageRole,
        fault_name: Option<String>,
    ) -> Result<Self, WsdlMessageOptionsError> {
        let contract_file = required(
            contract_file.into(),
            WsdlMessageOptionsError::EmptyContractFile,
        )?;
        let service = required(service.into(), WsdlMessageOptionsError::EmptyService)?;
        let port = required(port.into(), WsdlMessageOptionsError::EmptyPort)?;
        let operation = required(operation.into(), WsdlMessageOptionsError::EmptyOperation)?;
        let fault_name = fault_name.and_then(nonempty);
        match (role, fault_name.is_some()) {
            (WsdlMessageRole::Fault, false) => {
                return Err(WsdlMessageOptionsError::MissingFaultName);
            }
            (WsdlMessageRole::Fault, true) | (_, false) => {}
            (_, true) => return Err(WsdlMessageOptionsError::UnexpectedFaultName),
        }
        Ok(Self {
            contract_file,
            service,
            port,
            operation,
            role,
            fault_name,
        })
    }

    pub fn request(
        contract_file: impl Into<String>,
        service: impl Into<String>,
        port: impl Into<String>,
        operation: impl Into<String>,
    ) -> Result<Self, WsdlMessageOptionsError> {
        Self::new(
            contract_file,
            service,
            port,
            operation,
            WsdlMessageRole::Request,
            None,
        )
    }

    pub fn response(
        contract_file: impl Into<String>,
        service: impl Into<String>,
        port: impl Into<String>,
        operation: impl Into<String>,
    ) -> Result<Self, WsdlMessageOptionsError> {
        Self::new(
            contract_file,
            service,
            port,
            operation,
            WsdlMessageRole::Response,
            None,
        )
    }

    pub fn fault(
        contract_file: impl Into<String>,
        service: impl Into<String>,
        port: impl Into<String>,
        operation: impl Into<String>,
        fault_name: impl Into<String>,
    ) -> Result<Self, WsdlMessageOptionsError> {
        Self::new(
            contract_file,
            service,
            port,
            operation,
            WsdlMessageRole::Fault,
            Some(fault_name.into()),
        )
    }

    pub fn contract_file(&self) -> &str {
        &self.contract_file
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    pub fn port(&self) -> &str {
        &self.port
    }

    pub fn operation(&self) -> &str {
        &self.operation
    }

    pub const fn role(&self) -> WsdlMessageRole {
        self.role
    }

    pub fn fault_name(&self) -> Option<&str> {
        self.fault_name.as_deref()
    }

    pub fn same_contract(&self, other: &Self) -> bool {
        self.contract_file == other.contract_file
            && self.service == other.service
            && self.port == other.port
            && self.operation == other.operation
    }
}

fn required(
    value: String,
    error: WsdlMessageOptionsError,
) -> Result<String, WsdlMessageOptionsError> {
    nonempty(value).ok_or(error)
}

fn nonempty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[derive(Deserialize)]
struct SerializedWsdlMessageOptions {
    contract_file: String,
    service: String,
    port: String,
    operation: String,
    role: WsdlMessageRole,
    #[serde(default)]
    fault_name: Option<String>,
}

impl<'de> Deserialize<'de> for WsdlMessageOptions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let serialized = SerializedWsdlMessageOptions::deserialize(deserializer)?;
        Self::new(
            serialized.contract_file,
            serialized.service,
            serialized.port,
            serialized.operation,
            serialized.role,
            serialized.fault_name,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_confine_fault_names() -> Result<(), Box<dyn Error>> {
        let request = WsdlMessageOptions::request(
            "catalog.wsdl",
            "{urn:catalog}CatalogService",
            "CatalogPort",
            "{urn:catalog}Lookup",
        )?;
        assert_eq!(request.role(), WsdlMessageRole::Request);
        assert_eq!(request.fault_name(), None);

        assert_eq!(
            WsdlMessageOptions::fault(
                "catalog.wsdl",
                "{urn:catalog}CatalogService",
                "CatalogPort",
                "{urn:catalog}Lookup",
                " "
            ),
            Err(WsdlMessageOptionsError::MissingFaultName)
        );
        Ok(())
    }

    #[test]
    fn deserialization_validates_the_contract() {
        let result = serde_json::from_str::<WsdlMessageOptions>(
            r#"{"contract_file":"","service":"S","port":"P","operation":"O","role":"request"}"#,
        );
        assert!(result.is_err());
    }
}
