//! Polymorphic type schemes for ops.
//! The type scheme declares a number of TypeParams; any TypeArgs fitting those,
//! produce a FunctionType for the Op by substitution.

use crate::types::type_param::{check_type_args, TypeArg, TypeParam};
use crate::types::FunctionType;

use super::{CustomSignatureFunc, ExtensionRegistry, SignatureError};

/// A polymorphic type scheme for an op
pub struct OpDefTypeScheme<'a> {
    /// The declared type parameters, i.e., every Op must provide [TypeArg]s for these
    pub params: Vec<TypeParam>,
    /// Template for the Op type. May contain variables up to length of [OpDefTypeScheme::params]
    body: FunctionType,
    /// Extensions - the [TypeDefBound]s in here will be needed when we instantiate the [OpDefTypeScheme]
    /// into a [FunctionType].
    ///
    /// [TypeDefBound]: super::type_def::TypeDefBound
    // Note that if the lifetimes, etc., become too painful to store this reference in here,
    // and we'd rather own the necessary data, we really only need the TypeDefBounds not the other parts,
    // and the validation traversal in new() discovers the small subset of TypeDefBounds that
    // each OpDefTypeScheme actually needs.
    exts: &'a ExtensionRegistry,
}

impl<'a> OpDefTypeScheme<'a> {
    /// Create a new OpDefTypeScheme.
    ///
    /// #Errors
    /// Validates that all types in the schema are well-formed and all variables in the body
    /// are declared with [TypeParam]s that guarantee they will fit.
    pub fn new(
        params: impl Into<Vec<TypeParam>>,
        body: FunctionType,
        extension_registry: &'a ExtensionRegistry,
    ) -> Result<Self, SignatureError> {
        let params = params.into();
        body.validate(extension_registry, &params)?;
        Ok(Self {
            params,
            body,
            exts: extension_registry,
        })
    }
}

impl<'a> CustomSignatureFunc for OpDefTypeScheme<'a> {
    fn compute_signature(
        &self,
        _name: &smol_str::SmolStr,
        args: &[TypeArg],
        _misc: &std::collections::HashMap<String, serde_yaml::Value>,
    ) -> Result<FunctionType, SignatureError> {
        check_type_args(args, &self.params).map_err(SignatureError::TypeArgMismatch)?;
        Ok(self.body.substitute(self.exts, args))
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::num::NonZeroU64;

    use smol_str::SmolStr;

    use crate::extension::prelude::USIZE_T;
    use crate::extension::{
        CustomSignatureFunc, ExtensionId, ExtensionRegistry, SignatureError, TypeDefBound, PRELUDE,
    };
    use crate::std_extensions::collections::{EXTENSION, LIST_TYPENAME};
    use crate::types::type_param::{TypeArg, TypeArgError, TypeParam};
    use crate::types::{CustomType, FunctionType, Type, TypeBound};
    use crate::Extension;

    use super::OpDefTypeScheme;

    #[test]
    fn test_opaque() -> Result<(), SignatureError> {
        let list_def = EXTENSION.get_type(&LIST_TYPENAME).unwrap();
        let tyvar = TypeArg::use_var(0, TypeParam::Type(TypeBound::Any));
        let list_of_var = Type::new_extension(list_def.instantiate_concrete([tyvar.clone()])?);
        let reg: ExtensionRegistry = [PRELUDE.to_owned(), EXTENSION.to_owned()].into();
        let list_len = OpDefTypeScheme::new(
            [TypeParam::Type(TypeBound::Any)],
            FunctionType::new(vec![list_of_var], vec![USIZE_T]),
            &reg,
        )?;

        let t = list_len.compute_signature(
            &SmolStr::new_inline(""),
            &[TypeArg::Type { ty: USIZE_T }],
            &HashMap::new(),
        )?;
        assert_eq!(
            t,
            FunctionType::new(
                vec![Type::new_extension(
                    list_def
                        .instantiate_concrete([TypeArg::Type { ty: USIZE_T }])
                        .unwrap()
                )],
                vec![USIZE_T]
            )
        );

        Ok(())
    }

    fn id_fn(t: Type) -> FunctionType {
        FunctionType::new(vec![t.clone()], vec![t])
    }

    #[test]
    fn test_mismatched_args() -> Result<(), SignatureError> {
        const ARRAY_EXT_ID: ExtensionId = ExtensionId::new_unchecked("array_ext");
        const ARRAY_TYPE_NAME: SmolStr = SmolStr::new_inline("Array");

        let mut e = Extension::new(ARRAY_EXT_ID);
        e.add_type(
            ARRAY_TYPE_NAME,
            vec![TypeParam::Type(TypeBound::Any), TypeParam::max_nat()],
            "elemtype and size".to_string(),
            TypeDefBound::FromParams(vec![0]),
        )
        .unwrap();

        let reg: ExtensionRegistry = [e, PRELUDE.to_owned()].into();
        let ar_def = reg
            .get(&ARRAY_EXT_ID)
            .unwrap()
            .get_type(&ARRAY_TYPE_NAME)
            .unwrap();
        let typarams = [TypeParam::Type(TypeBound::Any), TypeParam::max_nat()];
        let tyvar = TypeArg::use_var(0, typarams[0].clone());
        let szvar = TypeArg::use_var(1, typarams[1].clone());

        // Valid schema...
        let good_array =
            Type::new_extension(ar_def.instantiate_concrete([tyvar.clone(), szvar.clone()])?);
        let good_ts = OpDefTypeScheme::new(typarams.clone(), id_fn(good_array), &reg)?;

        // Sanity check (good args)
        good_ts.compute_signature(
            &"reverse".into(),
            &[TypeArg::Type { ty: USIZE_T }, TypeArg::BoundedNat { n: 5 }],
            &HashMap::new(),
        )?;

        let wrong_args = good_ts.compute_signature(
            &"reverse".into(),
            &[TypeArg::BoundedNat { n: 5 }, TypeArg::Type { ty: USIZE_T }],
            &HashMap::new(),
        );
        assert_eq!(
            wrong_args,
            Err(SignatureError::TypeArgMismatch(
                TypeArgError::TypeMismatch {
                    param: typarams[0].clone(),
                    arg: TypeArg::BoundedNat { n: 5 }
                }
            ))
        );

        // (Try to) make a schema with bad args
        let arg_err = SignatureError::TypeArgMismatch(TypeArgError::TypeMismatch {
            param: typarams[0].clone(),
            arg: szvar.clone(),
        });
        assert_eq!(
            ar_def.instantiate_concrete([szvar.clone(), tyvar.clone()]),
            Err(arg_err.clone())
        );
        // ok, so that doesn't work - well, it shouldn't! So let's say we just have this signature (with bad args)...
        let bad_array = Type::new_extension(CustomType::new(
            ARRAY_TYPE_NAME,
            [szvar, tyvar],
            ARRAY_EXT_ID,
            TypeBound::Any,
        ));
        let bad_ts = OpDefTypeScheme::new(typarams.clone(), id_fn(bad_array), &reg);
        assert_eq!(bad_ts.err(), Some(arg_err));

        Ok(())
    }

    #[test]
    fn test_misused_variables() -> Result<(), SignatureError> {
        // Variables in args have different bounds from variable declaration
        let tv = TypeArg::use_var(0, TypeParam::Type(TypeBound::Copyable));
        let list_def = EXTENSION.get_type(&LIST_TYPENAME).unwrap();
        let body_type = id_fn(Type::new_extension(list_def.instantiate_concrete([tv])?));
        let reg = [EXTENSION.to_owned()].into();
        for decl in [
            TypeParam::Extensions,
            TypeParam::List(Box::new(TypeParam::max_nat())),
            TypeParam::Type(TypeBound::Any),
        ] {
            let invalid_ts = OpDefTypeScheme::new([decl.clone()], body_type.clone(), &reg);
            assert_eq!(
                invalid_ts.err(),
                Some(SignatureError::TypeVarDoesNotMatchDeclaration {
                    used: TypeParam::Type(TypeBound::Copyable),
                    decl: Some(decl)
                })
            );
        }
        // Variable not declared at all
        let invalid_ts = OpDefTypeScheme::new([], body_type, &reg);
        assert_eq!(
            invalid_ts.err(),
            Some(SignatureError::TypeVarDoesNotMatchDeclaration {
                used: TypeParam::Type(TypeBound::Copyable),
                decl: None
            })
        );

        Ok(())
    }

    fn decl_accepts_rejects_var(
        bound: TypeParam,
        accepted: &[TypeParam],
        rejected: &[TypeParam],
    ) -> Result<(), SignatureError> {
        const EXT_ID: ExtensionId = ExtensionId::new_unchecked("my_ext");
        const TYPE_NAME: SmolStr = SmolStr::new_inline("MyType");

        let mut e = Extension::new(EXT_ID);
        e.add_type(
            TYPE_NAME,
            vec![bound.clone()],
            "".into(),
            TypeDefBound::Explicit(TypeBound::Any),
        )
        .unwrap();

        let reg: ExtensionRegistry = [e].into();

        let make_scheme = |tp: TypeParam| {
            OpDefTypeScheme::new(
                [tp.clone()],
                id_fn(Type::new_extension(CustomType::new(
                    TYPE_NAME,
                    [TypeArg::use_var(0, tp)],
                    EXT_ID,
                    TypeBound::Any,
                ))),
                &reg,
            )
        };
        for decl in accepted {
            make_scheme(decl.clone())?;
        }
        for decl in rejected {
            assert_eq!(
                make_scheme(decl.clone()).err(),
                Some(SignatureError::TypeArgMismatch(
                    TypeArgError::TypeMismatch {
                        param: bound.clone(),
                        arg: TypeArg::use_var(0, decl.clone())
                    }
                ))
            );
        }
        Ok(())
    }

    #[test]
    fn test_bound_covariance() -> Result<(), SignatureError> {
        decl_accepts_rejects_var(
            TypeParam::Type(TypeBound::Copyable),
            &[
                TypeParam::Type(TypeBound::Copyable),
                TypeParam::Type(TypeBound::Eq),
            ],
            &[TypeParam::Type(TypeBound::Any)],
        )?;

        let list_of_tys = |b| TypeParam::List(Box::new(TypeParam::Type(b)));
        decl_accepts_rejects_var(
            list_of_tys(TypeBound::Copyable),
            &[list_of_tys(TypeBound::Copyable), list_of_tys(TypeBound::Eq)],
            &[list_of_tys(TypeBound::Any)],
        )?;

        decl_accepts_rejects_var(
            TypeParam::max_nat(),
            &[TypeParam::bounded_nat(NonZeroU64::new(5).unwrap())],
            &[],
        )?;
        decl_accepts_rejects_var(
            TypeParam::bounded_nat(NonZeroU64::new(10).unwrap()),
            &[TypeParam::bounded_nat(NonZeroU64::new(5).unwrap())],
            &[TypeParam::max_nat()],
        )?;
        Ok(())
    }
}