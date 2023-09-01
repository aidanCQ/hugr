//! Opaque types, used to represent a user-defined [`Type`].
//!
//! [`Type`]: super::Type
use smol_str::SmolStr;
use std::fmt::{self, Display};

use crate::extension::{ExtensionId, ExtensionRegistry, SignatureError};

use super::{
    type_param::{TypeArg, TypeParam},
    TypeBound,
};

/// An opaque type element. Contains the unique identifier of its definition.
#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomType {
    extension: ExtensionId,
    /// Unique identifier of the opaque type.
    /// Same as the corresponding [`TypeDef`]
    ///
    /// [`TypeDef`]: crate::extension::TypeDef
    id: SmolStr,
    /// Arguments that fit the [`TypeParam`]s declared by the typedef
    ///
    /// [`TypeParam`]: super::type_param::TypeParam
    args: Vec<TypeArg>,
    /// The [TypeBound] describing what can be done to instances of this type
    bound: TypeBound,
}

impl CustomType {
    /// Creates a new opaque type.
    pub fn new(
        id: impl Into<SmolStr>,
        args: impl Into<Vec<TypeArg>>,
        extension: impl Into<ExtensionId>,
        bound: TypeBound,
    ) -> Self {
        Self {
            id: id.into(),
            args: args.into(),
            extension: extension.into(),
            bound,
        }
    }

    /// Creates a new opaque type (constant version, no type arguments)
    pub const fn new_simple(id: SmolStr, extension: ExtensionId, bound: TypeBound) -> Self {
        Self {
            id,
            args: vec![],
            extension,
            bound,
        }
    }

    /// Returns the bound of this [`CustomType`].
    pub const fn bound(&self) -> TypeBound {
        self.bound
    }

    pub(super) fn validate(
        &self,
        extension_registry: &ExtensionRegistry,
        type_vars: &[TypeParam],
    ) -> Result<(), SignatureError> {
        // Check the args are individually ok
        self.args
            .iter()
            .try_for_each(|a| a.validate(extension_registry, type_vars))?;
        // And check they fit into the TypeParams declared by the TypeDef
        let ex = extension_registry.get(&self.extension);
        // Even if OpDef's (+binaries) are not available, the part of the Extension definition
        // describing the TypeDefs can easily be passed around (serialized), so should be available.
        let ex = ex.ok_or(SignatureError::ExtensionNotFound(self.extension.clone()))?;
        let def = ex
            .get_type(&self.id)
            .ok_or(SignatureError::ExtensionTypeNotFound {
                exn: self.extension.clone(),
                typ: self.id.clone(),
            })?;
        def.check_custom(self)
    }

    pub(super) fn substitute(&self, args: &[TypeArg]) -> Self {
        Self {
            args: self.args.iter().map(|arg| arg.substitute(args)).collect(),
            ..self.clone()
        }
        // TODO the bound could get narrower as a result of substitution.
        // But, we need the TypeDefBound (from the TypeDef in the Extension) to recalculate correctly...
    }
}

impl CustomType {
    /// unique name of the type.
    pub fn name(&self) -> &SmolStr {
        &self.id
    }

    /// Type arguments.
    pub fn args(&self) -> &[TypeArg] {
        &self.args
    }

    /// Parent extension.
    pub fn extension(&self) -> &ExtensionId {
        &self.extension
    }
}

impl Display for CustomType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}({:?})", self.id, self.args)
    }
}
