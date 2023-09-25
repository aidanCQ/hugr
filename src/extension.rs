//! Extensions
//!
//! TODO: YAML declaration and parsing. This should be similar to a plugin
//! system (outside the `types` module), which also parses nested [`OpDef`]s.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;

use smol_str::SmolStr;
use thiserror::Error;

use crate::hugr::IdentList;
use crate::ops;
use crate::ops::custom::{ExtensionOp, OpaqueOp};
use crate::types::type_param::{check_type_args, TypeArgError};
use crate::types::type_param::{TypeArg, TypeParam};
use crate::types::{check_typevar_decl, CustomType, PolyFuncType, Substitution, TypeBound};

mod infer;
pub use infer::{infer_extensions, ExtensionSolution, InferExtensionError};

mod op_def;
pub use op_def::{CustomSignatureFunc, OpDef};
mod type_def;
pub use type_def::{TypeDef, TypeDefBound};
pub mod prelude;
pub mod validate;

pub use prelude::{PRELUDE, PRELUDE_REGISTRY};

/// Extension Registries store extensions to be looked up e.g. during validation.
#[derive(Clone, Debug)]
pub struct ExtensionRegistry(BTreeMap<ExtensionId, Extension>);

impl ExtensionRegistry {
    /// Makes a new (empty) registry.
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Gets the Extension with the given name
    pub fn get(&self, name: &str) -> Option<&Extension> {
        self.0.get(name)
    }
}

/// An Extension Registry containing no extensions.
pub const EMPTY_REG: ExtensionRegistry = ExtensionRegistry::new();

impl<T: IntoIterator<Item = Extension>> From<T> for ExtensionRegistry {
    fn from(value: T) -> Self {
        let mut reg = Self::new();
        for ext in value.into_iter() {
            let prev = reg.0.insert(ext.name.clone(), ext);
            if let Some(prev) = prev {
                panic!("Multiple extensions with same name: {}", prev.name)
            };
        }
        reg
    }
}

/// An error that can occur in computing the signature of a node.
/// TODO: decide on failure modes
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum SignatureError {
    /// Name mismatch
    #[error("Definition name ({0}) and instantiation name ({1}) do not match.")]
    NameMismatch(SmolStr, SmolStr),
    /// Extension mismatch
    #[error("Definition extension ({0:?}) and instantiation extension ({1:?}) do not match.")]
    ExtensionMismatch(ExtensionId, ExtensionId),
    /// When the type arguments of the node did not match the params declared by the OpDef
    #[error("Type arguments of node did not match params declared by definition: {0}")]
    TypeArgMismatch(#[from] TypeArgError),
    /// Invalid type arguments
    #[error("Invalid type arguments for operation")]
    InvalidTypeArgs,
    /// The Extension Registry did not contain an Extension referenced by the Signature
    #[error("Extension '{0}' not found")]
    ExtensionNotFound(ExtensionId),
    /// The Extension was found in the registry, but did not contain the Type(Def) referenced in the Signature
    #[error("Extension '{exn}' did not contain expected TypeDef '{typ}'")]
    ExtensionTypeNotFound { exn: ExtensionId, typ: SmolStr },
    /// The bound recorded for a CustomType doesn't match what the TypeDef would compute
    #[error("Bound on CustomType ({actual}) did not match TypeDef ({expected})")]
    WrongBound {
        actual: TypeBound,
        expected: TypeBound,
    },
    /// A Type Variable is used as a kind that does not match the declaration
    #[error("Type Variable used as {used:?} but declared as {decl:?}")]
    TypeVarDoesNotMatchDeclaration { used: TypeParam, decl: TypeParam },
    /// A type variable that was used has not been declared
    #[error("Type variable {idx} was not declared ({num_decls} in scope)")]
    FreeTypeVar { idx: usize, num_decls: usize },
    /// The type stored in a [LeafOp::TypeApply] is not what we compute from the
    /// [ExtensionRegistry]. (Note: might be commoned up with
    /// [CustomOpError::SignatureMismatch] if we implement
    /// <https://github.com/CQCL-DEV/hugr/issues/508>).
    ///
    /// [LeafOp::TypeApply]: crate::ops::LeafOp::TypeApply
    /// [CustomOpError::SignatureMismatch]: crate::ops::custom::CustomOpError::SignatureMismatch
    #[error("Incorrect cache of signature - found {stored} but expected {expected}")]
    CachedTypeIncorrect {
        stored: PolyFuncType,
        expected: PolyFuncType,
    },
}

/// Concrete instantiations of types and operations defined in extensions.
trait CustomConcrete {
    fn def_name(&self) -> &SmolStr;
    fn type_args(&self) -> &[TypeArg];
    fn parent_extension(&self) -> &ExtensionId;
}

impl CustomConcrete for OpaqueOp {
    fn def_name(&self) -> &SmolStr {
        self.name()
    }

    fn type_args(&self) -> &[TypeArg] {
        self.args()
    }

    fn parent_extension(&self) -> &ExtensionId {
        self.extension()
    }
}

impl CustomConcrete for CustomType {
    fn def_name(&self) -> &SmolStr {
        self.name()
    }

    fn type_args(&self) -> &[TypeArg] {
        self.args()
    }

    fn parent_extension(&self) -> &ExtensionId {
        self.extension()
    }
}

/// Type-parametrised functionality shared between [`TypeDef`] and [`OpDef`].
trait TypeParametrised {
    /// The concrete object built by binding type arguments to parameters
    type Concrete: CustomConcrete;
    /// The extension-unique name.
    fn name(&self) -> &SmolStr;
    /// Type parameters.
    fn params(&self) -> &[TypeParam];
    /// The parent extension.
    fn extension(&self) -> &ExtensionId;
    /// Check provided type arguments are valid against parameters.
    fn check_args_impl(&self, args: &[TypeArg]) -> Result<(), SignatureError> {
        check_type_args(args, self.params()).map_err(SignatureError::TypeArgMismatch)
    }

    /// Check custom instance is a valid instantiation of this definition.
    ///
    /// # Errors
    ///
    /// This function will return an error if the type of the instance does not
    /// match the definition.
    fn check_concrete_impl(&self, custom: &Self::Concrete) -> Result<(), SignatureError> {
        if self.extension() != custom.parent_extension() {
            return Err(SignatureError::ExtensionMismatch(
                self.extension().clone(),
                custom.parent_extension().clone(),
            ));
        }
        if self.name() != custom.def_name() {
            return Err(SignatureError::NameMismatch(
                self.name().clone(),
                custom.def_name().clone(),
            ));
        }

        self.check_args_impl(custom.type_args())?;

        Ok(())
    }
}

/// A constant value provided by a extension.
/// Must be an instance of a type available to the extension.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExtensionValue {
    extension: ExtensionId,
    name: SmolStr,
    typed_value: ops::Const,
}

impl ExtensionValue {
    /// Returns a reference to the typed value of this [`ExtensionValue`].
    pub fn typed_value(&self) -> &ops::Const {
        &self.typed_value
    }

    /// Returns a reference to the name of this [`ExtensionValue`].
    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    /// Returns a reference to the extension this [`ExtensionValue`] belongs to.
    pub fn extension(&self) -> &ExtensionId {
        &self.extension
    }
}

/// A unique identifier for a extension.
///
/// The actual [`Extension`] is stored externally.
pub type ExtensionId = IdentList;

/// A extension is a set of capabilities required to execute a graph.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Extension {
    /// Unique identifier for the extension.
    pub name: ExtensionId,
    /// Other extensions defining types used by this extension.
    /// That is, an upper-bound on the types that can be returned by
    /// computing the signature of any operation in this extension,
    /// for any possible [TypeArg].
    pub extension_reqs: ExtensionSet,
    /// Types defined by this extension.
    types: HashMap<SmolStr, TypeDef>,
    /// Static values defined by this extension.
    values: HashMap<SmolStr, ExtensionValue>,
    /// Operation declarations with serializable definitions.
    // Note: serde will serialize this because we configure with `features=["rc"]`.
    // That will clone anything that has multiple references, but each
    // OpDef should appear exactly once in this map (keyed by its name),
    // and the other references to the OpDef are from ExternalOp's in the Hugr
    // (which are serialized as OpaqueOp's i.e. Strings).
    operations: HashMap<SmolStr, Arc<op_def::OpDef>>,
}

impl Extension {
    /// Creates a new extension with the given name.
    pub fn new(name: ExtensionId) -> Self {
        Self::new_with_reqs(name, Default::default())
    }

    /// Creates a new extension with the given name and requirements.
    pub fn new_with_reqs(name: ExtensionId, extension_reqs: ExtensionSet) -> Self {
        Self {
            name,
            extension_reqs,
            types: Default::default(),
            values: Default::default(),
            operations: Default::default(),
        }
    }

    /// Allows read-only access to the operations in this Extension
    pub fn get_op(&self, op_name: &str) -> Option<&Arc<op_def::OpDef>> {
        self.operations.get(op_name)
    }

    /// Allows read-only access to the types in this Extension
    pub fn get_type(&self, type_name: &str) -> Option<&type_def::TypeDef> {
        self.types.get(type_name)
    }

    /// Allows read-only access to the values in this Extension
    pub fn get_value(&self, type_name: &str) -> Option<&ExtensionValue> {
        self.values.get(type_name)
    }

    /// Returns the name of the extension.
    pub fn name(&self) -> &ExtensionId {
        &self.name
    }

    /// Iterator over the operations of this [`Extension`].
    pub fn operations(&self) -> impl Iterator<Item = (&SmolStr, &Arc<OpDef>)> {
        self.operations.iter()
    }

    /// Iterator over the types of this [`Extension`].
    pub fn types(&self) -> impl Iterator<Item = (&SmolStr, &TypeDef)> {
        self.types.iter()
    }

    /// Add a named static value to the extension.
    pub fn add_value(
        &mut self,
        name: impl Into<SmolStr>,
        typed_value: ops::Const,
    ) -> Result<&mut ExtensionValue, ExtensionBuildError> {
        let extension_value = ExtensionValue {
            extension: self.name.clone(),
            name: name.into(),
            typed_value,
        };
        match self.values.entry(extension_value.name.clone()) {
            Entry::Occupied(_) => Err(ExtensionBuildError::OpDefExists(extension_value.name)),
            Entry::Vacant(ve) => Ok(ve.insert(extension_value)),
        }
    }

    /// Instantiate an [`ExtensionOp`] which references an [`OpDef`] in this extension.
    pub fn instantiate_extension_op(
        &self,
        op_name: &str,
        args: impl Into<Vec<TypeArg>>,
        ext_reg: &ExtensionRegistry,
    ) -> Result<ExtensionOp, SignatureError> {
        let op_def = self.get_op(op_name).expect("Op not found.");
        ExtensionOp::new(op_def.clone(), args, ext_reg)
    }
}

impl PartialEq for Extension {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

/// An error that can occur in computing the signature of a node.
/// TODO: decide on failure modes
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ExtensionBuildError {
    /// Existing [`OpDef`]
    #[error("Extension already has an op called {0}.")]
    OpDefExists(SmolStr),
    /// Existing [`TypeDef`]
    #[error("Extension already has an type called {0}.")]
    TypeDefExists(SmolStr),
}

/// A set of extensions identified by their unique [`ExtensionId`].
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExtensionSet(HashSet<ExtensionId>);

impl ExtensionSet {
    /// Creates a new empty extension set.
    pub fn new() -> Self {
        Self(HashSet::new())
    }

    /// Creates a new extension set from some extensions.
    pub fn new_from_extensions(extensions: impl Into<HashSet<ExtensionId>>) -> Self {
        Self(extensions.into())
    }

    /// Adds a extension to the set.
    pub fn insert(&mut self, extension: &ExtensionId) {
        self.0.insert(extension.clone());
    }

    /// Adds a type var (which must have been declared as a [TypeParam::Extensions]) to this set
    pub fn insert_type_var(&mut self, idx: usize) {
        // Represent type vars as string representation of DeBruijn index.
        // This is not a legal IdentList or ExtensionId so should not conflict.
        self.0
            .insert(ExtensionId::new_unchecked(idx.to_string().as_str()));
    }

    /// Returns `true` if the set contains the given extension.
    pub fn contains(&self, extension: &ExtensionId) -> bool {
        self.0.contains(extension)
    }

    /// Returns `true` if the set is a subset of `other`.
    pub fn is_subset(&self, other: &Self) -> bool {
        self.0.is_subset(&other.0)
    }

    /// Returns `true` if the set is a superset of `other`.
    pub fn is_superset(&self, other: &Self) -> bool {
        self.0.is_superset(&other.0)
    }

    /// Create a extension set with a single element.
    pub fn singleton(extension: &ExtensionId) -> Self {
        let mut set = Self::new();
        set.insert(extension);
        set
    }

    /// An ExtensionSet containing a single type variable
    /// (which must have been declared as a [TypeParam::Extensions])
    pub fn type_var(idx: usize) -> Self {
        let mut set = Self::new();
        set.insert_type_var(idx);
        set
    }

    /// Returns the union of two extension sets.
    pub fn union(mut self, other: &Self) -> Self {
        self.0.extend(other.0.iter().cloned());
        self
    }

    /// The things in other which are in not in self
    pub fn missing_from(&self, other: &Self) -> Self {
        ExtensionSet(HashSet::from_iter(other.0.difference(&self.0).cloned()))
    }

    /// Iterate over the contained ExtensionIds
    pub fn iter(&self) -> impl Iterator<Item = &ExtensionId> {
        self.0.iter()
    }

    pub(crate) fn validate(&self, params: &[TypeParam]) -> Result<(), SignatureError> {
        self.iter()
            .filter_map(as_typevar)
            .try_for_each(|var_idx| check_typevar_decl(params, var_idx, &TypeParam::Extensions))
    }

    pub(crate) fn substitute(&self, sub: &Substitution) -> Self {
        Self::from_iter(self.0.iter().flat_map(|e| match as_typevar(e) {
            None => vec![e.clone()],
            Some(i) => match sub.apply_var(i, &TypeParam::Extensions) {
                TypeArg::Extensions{es} => es.iter().cloned().collect::<Vec<_>>(),
                _ => panic!("value for type var was not extension set - type scheme should be validate()d first"),
            },
        }))
    }
}

fn as_typevar(e: &ExtensionId) -> Option<usize> {
    // Type variables are represented as radix-10 numbers, which are illegal
    // as standard ExtensionIds. Hence if an ExtensionId starts with a digit,
    // we assume it must be a type variable, and fail fast if it isn't.
    match e.chars().next() {
        Some(c) if c.is_ascii_digit() => Some(str::parse(e).unwrap()),
        _ => None,
    }
}

impl Display for ExtensionSet {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        f.debug_list().entries(self.0.iter()).finish()
    }
}

impl FromIterator<ExtensionId> for ExtensionSet {
    fn from_iter<I: IntoIterator<Item = ExtensionId>>(iter: I) -> Self {
        Self(HashSet::from_iter(iter))
    }
}
