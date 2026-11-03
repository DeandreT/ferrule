use std::cell::RefCell;
use std::rc::Rc;

use crate::{RuntimeError, Value};

use super::ScopeContext;

pub(super) type ProjectionState = Rc<RefCell<functions::JsonArrayProjections>>;

impl ScopeContext<'_> {
    /// Calls a graph-level JSON array projection in this invocation. The
    /// source ID belongs to the generated mapping's main graph only;
    /// user-function and child-program graphs use their own ordinary dispatch.
    pub fn call_json_array_projection(
        &self,
        source: u32,
        function: &str,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.json_projections
            .borrow_mut()
            .call(functions::JsonProjectionOwner { source }, function, args)
            .map_err(RuntimeError::from)
    }
}
