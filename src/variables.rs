use std::fmt;
use std::error::Error;
use std::sync::Arc;

use edgedb_protocol::value::Value;
use edgedb_protocol::codec;
use edgedb_protocol::descriptors::{InputTypedesc, Descriptor};
use crate::repl;
use crate::prompt;
use crate::prompt::variable::{self, VariableInput};


#[derive(Debug)]
pub struct Canceled;


pub async fn input_variables(desc: &InputTypedesc, state: &mut repl::PromptRpc)
    -> Result<Value, anyhow::Error>
{
    // only for protocol < 0.12
    if desc.is_empty_tuple() {
        return Ok(Value::Tuple(Vec::new()));
    }
    match desc.root() {
        Some(Descriptor::Tuple(tuple)) if desc.proto().is_at_most(0, 11) => {
            let mut val = Vec::with_capacity(tuple.element_types.len());
            for (idx, el) in tuple.element_types.iter().enumerate() {
                val.push(input_item(
                    &format!("{}", idx), desc.get(*el)?, desc, state, false,
                ).await?.expect("no optional"));
            }
            return Ok(Value::Tuple(val));
        }
        Some(Descriptor::NamedTuple(tuple)) if desc.proto().is_at_most(0, 11)
        => {
            let mut fields = Vec::with_capacity(tuple.elements.len());
            let shape = tuple.elements[..].into();
            for el in tuple.elements.iter() {
                fields.push(input_item(
                    &el.name, desc.get(el.type_pos)?, desc, state, false
                ).await?.expect("no optional"));
            }
            return Ok(Value::NamedTuple { shape, fields });
        }
        Some(Descriptor::ObjectShape(obj)) if desc.proto().is_at_least(0, 12)
        => {
            let mut fields = Vec::with_capacity(obj.elements.len());
            let shape = obj.elements[..].into();
            for el in obj.elements.iter() {
                let optional = el.cardinality
                    .map(|c| c.is_optional()).unwrap_or(false);
                fields.push(input_item(
                    &el.name, desc.get(el.type_pos)?, desc, state, optional,
                ).await?);
            }
            return Ok(Value::Object { shape, fields });
        }
        Some(root) => {
            return Err(anyhow::anyhow!(
                "Unknown input type descriptor: {:?}", root));
        }
        // Since protocol 0.12
        None => {
            return Ok(Value::Nothing);
        }
    }
}

fn make_variable_input(item: &Descriptor, all: &InputTypedesc)
                       -> Result<Arc<dyn VariableInput>, anyhow::Error>
{
    let citem = match item {
        Descriptor::Scalar(s) => {
            all.get(s.base_type_pos)?
        }
        _ => { item },
    };
    match citem {
        Descriptor::BaseScalar(s) => {
            let var_type: Arc<dyn VariableInput> = match s.id {
                codec::STD_STR => Arc::new(variable::Str),
                codec::STD_UUID => Arc::new(variable::Uuid),
                codec::STD_INT16 => Arc::new(variable::Int16),
                codec::STD_INT32 => Arc::new(variable::Int32),
                codec::STD_INT64 => Arc::new(variable::Int64),
                codec::STD_FLOAT32 => Arc::new(variable::Float32),
                codec::STD_FLOAT64 => Arc::new(variable::Float64),
                codec::STD_DECIMAL => Arc::new(variable::Decimal),
                codec::STD_BOOL => Arc::new(variable::Bool),
                codec::STD_JSON => Arc::new(variable::Json),
                codec::STD_BIGINT => Arc::new(variable::BigInt),
                _ => return Err(anyhow::anyhow!(
                        "Unimplemented input type {}", s.id))
            };

            Ok(var_type)
        }
        Descriptor::Array(s) => {
            // XXX: dimensions
            let elem = all.get(s.type_pos)?;
            Ok(Arc::new(variable::Array::new(make_variable_input(elem, all)?)))
        }
        _ => Err(anyhow::anyhow!(
                "Unimplemented input type descriptor: {:?}", item)),
    }
}

async fn input_item(name: &str, item: &Descriptor, all: &InputTypedesc,
    state: &mut repl::PromptRpc, optional: bool)
    -> Result<Option<Value>, anyhow::Error>
{
    let var_type: Arc<dyn VariableInput> = make_variable_input(item, all)?;
    let val = match
        state.variable_input(name, var_type, optional, "").await?
    {
        | prompt::Input::Value(val) => Some(val),
        | prompt::Input::Text(_) => unreachable!(),
        | prompt::Input::Interrupt => Err(Canceled)?,
        | prompt::Input::Eof => None,
    };
    Ok(val)
}

impl Error for Canceled {
}

impl fmt::Display for Canceled {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        "Operation canceled".fmt(f)
    }
}
