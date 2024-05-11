//! Abstract and concrete Signature types.

use itertools::Either;

use std::fmt::{self, Display, Write};

use super::type_param::TypeParam;
use super::{Substitution, Type, TypeRow};

use crate::extension::{ExtensionRegistry, ExtensionSet, SignatureError};
use crate::{Direction, IncomingPort, OutgoingPort, Port};

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// Describes the edges required to/from a node, and thus, also the type of a [Graph].
/// This includes both the concept of "signature" in the spec,
/// and also the target (value) of a call (static).
///
/// [Graph]: crate::ops::constant::Value::Function
pub struct FunctionType<const ROWVARS:bool = false> {
    /// Value inputs of the function.
    pub input: TypeRow<ROWVARS>,
    /// Value outputs of the function.
    pub output: TypeRow<ROWVARS>,
    /// The extension requirements which are added by the operation
    pub extension_reqs: ExtensionSet,
}

impl <const RV:bool> FunctionType<RV> {
    /// Builder method, add extension_reqs to an FunctionType
    pub fn with_extension_delta(mut self, rs: impl Into<ExtensionSet>) -> Self {
        self.extension_reqs = self.extension_reqs.union(rs.into());
        self
    }
}

impl FunctionType<true> {
    pub(super) fn validate(
        &self,
        extension_registry: &ExtensionRegistry,
        var_decls: &[TypeParam],
    ) -> Result<(), SignatureError> {
        self.input.validate(extension_registry, var_decls)?;
        self.output
            .validate(extension_registry, var_decls)?;
        self.extension_reqs.validate(var_decls)
    }
}

impl <const RV:bool> FunctionType<RV>
{
    pub(crate) fn substitute(&self, tr: &Substitution) -> Self {
        FunctionType {
            input: self.input.substitute(tr),
            output: self.output.substitute(tr),
            extension_reqs: self.extension_reqs.substitute(tr),
        }
    }

    /// The number of wires in the signature.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.input.is_empty() && self.output.is_empty()
    }

    /// Create a new signature with specified inputs and outputs.
    pub fn new(input: impl Into<TypeRow<RV>>, output: impl Into<TypeRow<RV>>) -> Self {
        Self {
            input: input.into(),
            output: output.into(),
            extension_reqs: ExtensionSet::new(),
        }
    }
    /// Create a new signature with the same input and output types (signature of an endomorphic
    /// function).
    pub fn new_endo(linear: impl Into<TypeRow<RV>>) -> Self {
        let linear = linear.into();
        Self::new(linear.clone(), linear)
    }
}

impl FunctionType<false> {
    /// Returns the type of a value [`Port`]. Returns `None` if the port is out
    /// of bounds.
    #[inline]
    pub fn port_type(&self, port: impl Into<Port>) -> Option<&Type> {
        let port: Port = port.into();
        match port.as_directed() {
            Either::Left(port) => self.in_port_type(port),
            Either::Right(port) => self.out_port_type(port),
        }
    }

    /// Returns the type of a value input [`Port`]. Returns `None` if the port is out
    /// of bounds.
    #[inline]
    pub fn in_port_type(&self, port: impl Into<IncomingPort>) -> Option<&Type> {
        self.input.get(port.into())
    }

    /// Returns the type of a value output [`Port`]. Returns `None` if the port is out
    /// of bounds.
    #[inline]
    pub fn out_port_type(&self, port: impl Into<OutgoingPort>) -> Option<&Type> {
        self.output.get(port.into())
    }

    /// Returns a mutable reference to the type of a value input [`Port`]. Returns `None` if the port is out
    /// of bounds.
    #[inline]
    pub fn in_port_type_mut(&mut self, port: impl Into<IncomingPort>) -> Option<&mut Type> {
        self.input.get_mut(port.into())
    }

    /// Returns the type of a value output [`Port`]. Returns `None` if the port is out
    /// of bounds.
    #[inline]
    pub fn out_port_type_mut(&mut self, port: impl Into<OutgoingPort>) -> Option<&mut Type> {
        self.output.get_mut(port.into())
    }

    /// Returns a mutable reference to the type of a value [`Port`].
    /// Returns `None` if the port is out of bounds.
    #[inline]
    pub fn port_type_mut(&mut self, port: impl Into<Port>) -> Option<&mut Type> {
        let port = port.into();
        match port.as_directed() {
            Either::Left(port) => self.in_port_type_mut(port),
            Either::Right(port) => self.out_port_type_mut(port),
        }
    }

    /// Returns the number of ports in the signature.
    #[inline]
    pub fn port_count(&self, dir: Direction) -> usize {
        match dir {
            Direction::Incoming => self.input.len(),
            Direction::Outgoing => self.output.len(),
        }
    }

    /// Returns the number of input ports in the signature.
    #[inline]
    pub fn input_count(&self) -> usize {
        self.port_count(Direction::Incoming)
    }

    /// Returns the number of output ports in the signature.
    #[inline]
    pub fn output_count(&self) -> usize {
        self.port_count(Direction::Outgoing)
    }
}

impl <const RV:bool> FunctionType<RV> {
    /// Returns a slice of the types for the given direction.
    #[inline]
    pub fn types(&self, dir: Direction) -> &[Type<RV>] {
        match dir {
            Direction::Incoming => &self.input,
            Direction::Outgoing => &self.output,
        }
    }

    /// Returns a slice of the input types.
    #[inline]
    pub fn input_types(&self) -> &[Type<RV>] {
        self.types(Direction::Incoming)
    }

    /// Returns a slice of the output types.
    #[inline]
    pub fn output_types(&self) -> &[Type<RV>] {
        self.types(Direction::Outgoing)
    }

    #[inline]
    /// Returns the input row
    pub fn input(&self) -> &TypeRow<RV> {
        &self.input
    }

    #[inline]
    /// Returns the output row
    pub fn output(&self) -> &TypeRow<RV> {
        &self.output
    }
}

impl FunctionType<false> {
    /// Returns the `Port`s in the signature for a given direction.
    #[inline]
    pub fn ports(&self, dir: Direction) -> impl Iterator<Item = Port> {
        (0..self.port_count(dir)).map(move |i| Port::new(dir, i))
    }

    /// Returns the incoming `Port`s in the signature.
    #[inline]
    pub fn input_ports(&self) -> impl Iterator<Item = IncomingPort> {
        self.ports(Direction::Incoming)
            .map(|p| p.as_incoming().unwrap())
    }

    /// Returns the outgoing `Port`s in the signature.
    #[inline]
    pub fn output_ports(&self) -> impl Iterator<Item = OutgoingPort> {
        self.ports(Direction::Outgoing)
            .map(|p| p.as_outgoing().unwrap())
    }
}

impl From<FunctionType<false>> for FunctionType<true> {
    fn from(value: FunctionType<false>) -> Self {
        Self {
            input: value.input.into(),
            output: value.input.into(),
            extension_reqs: value.extension_reqs
        }
    }
}

impl TryFrom<FunctionType<true>> for FunctionType {
    type Error;

    fn try_from(value: FunctionType<true>) -> Result<Self, Self::Error> {
        Ok(Self {
            input: value.input.try_into()?,
            output: value.output.try_into()?,
            extension_reqs: value.extension_reqs
        })
    }
}

impl <const RV:bool> Display for FunctionType<RV> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.input.is_empty() {
            self.input.fmt(f)?;
            f.write_str(" -> ")?;
        }
        f.write_char('[')?;
        self.extension_reqs.fmt(f)?;
        f.write_char(']')?;
        self.output.fmt(f)
    }
}

#[cfg(test)]
mod test {
    use crate::{extension::prelude::USIZE_T, type_row};

    use super::*;
    #[test]
    fn test_function_type() {
        let mut f_type = FunctionType::new(type_row![Type::UNIT], type_row![Type::UNIT]);
        assert_eq!(f_type.input_count(), 1);
        assert_eq!(f_type.output_count(), 1);

        assert_eq!(f_type.input_types(), &[Type::UNIT]);

        assert_eq!(
            f_type.port_type(Port::new(Direction::Incoming, 0)),
            Some(&Type::UNIT)
        );

        let out = Port::new(Direction::Outgoing, 0);
        *(f_type.port_type_mut(out).unwrap()) = USIZE_T;

        assert_eq!(f_type.port_type(out), Some(&USIZE_T));

        assert_eq!(f_type.input_types(), &[Type::UNIT]);
        assert_eq!(f_type.output_types(), &[USIZE_T]);
    }
}
