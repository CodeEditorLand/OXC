#[allow(clippy::wildcard_imports)]
use oxc_ast::ast::*;

use oxc_allocator::Box;
use oxc_span::{GetSpan, SPAN};

use crate::{
    diagnostics::{
        accessor_must_have_explicit_return_type, computed_property_name, extends_clause_expression,
        method_must_have_explicit_return_type, property_must_have_explicit_type,
    },
    IsolatedDeclarations,
};

impl<'a> IsolatedDeclarations<'a> {
    pub fn is_literal_key(&self, key: &PropertyKey<'a>) -> bool {
        match key {
            PropertyKey::StringLiteral(_)
            | PropertyKey::NumericLiteral(_)
            | PropertyKey::BigintLiteral(_) => true,
            PropertyKey::TemplateLiteral(l) => l.expressions.is_empty(),
            PropertyKey::UnaryExpression(expr) => {
                expr.operator.is_arithmetic()
                    && matches!(
                        expr.argument,
                        Expression::NumericLiteral(_) | Expression::BigintLiteral(_)
                    )
            }
            _ => false,
        }
    }

    pub fn report_property_key(&self, key: &PropertyKey<'a>, computed: bool) -> bool {
        if computed && self.is_literal_key(key) {
            computed_property_name(key.span());
            true
        } else {
            false
        }
    }

    pub fn transform_accessibility(
        &self,
        accessibility: Option<TSAccessibility>,
    ) -> Option<TSAccessibility> {
        if accessibility.is_none() || accessibility.is_some_and(|a| a == TSAccessibility::Public) {
            None
        } else {
            accessibility
        }
    }

    pub fn transform_class_property_definition(
        &self,
        property: &PropertyDefinition<'a>,
    ) -> ClassElement<'a> {
        let type_annotations = if property.accessibility.is_some_and(|a| a.is_private()) {
            None
        } else {
            property
                .type_annotation
                .as_ref()
                .map(|type_annotation| self.ast.copy(type_annotation))
                .or_else(|| {
                    property
                        .value
                        .as_ref()
                        .and_then(|expr| {
                            let ts_type = self.infer_type_from_expression(expr);
                            if ts_type.is_none() {
                                self.error(property_must_have_explicit_type(property.key.span()));
                            }
                            ts_type
                        })
                        .map(|ts_type| self.ast.ts_type_annotation(SPAN, ts_type))
                })
        };

        self.ast.class_property(
            property.r#type,
            property.span,
            self.ast.copy(&property.key),
            None,
            property.computed,
            property.r#static,
            property.declare,
            property.r#override,
            property.optional,
            property.definite,
            property.readonly,
            type_annotations,
            self.transform_accessibility(property.accessibility),
            self.ast.new_vec(),
        )
    }

    pub fn transform_class_method_definition(
        &self,
        definition: &MethodDefinition<'a>,
        params: Box<'a, FormalParameters<'a>>,
    ) -> ClassElement<'a> {
        let function = &definition.value;
        if definition.accessibility.is_some_and(|a| a.is_private()) {
            let r#type = match definition.r#type {
                MethodDefinitionType::MethodDefinition => {
                    PropertyDefinitionType::PropertyDefinition
                }
                MethodDefinitionType::TSAbstractMethodDefinition => {
                    PropertyDefinitionType::TSAbstractPropertyDefinition
                }
            };
            return self.create_class_property(
                r#type,
                self.ast.copy(&definition.key),
                definition.r#override,
                self.transform_accessibility(definition.accessibility),
            );
        }

        let type_annotation = self.infer_function_return_type(function);

        if type_annotation.is_none() {
            match definition.kind {
                MethodDefinitionKind::Method => {
                    self.error(method_must_have_explicit_return_type(definition.key.span()));
                }
                MethodDefinitionKind::Get => {
                    self.error(accessor_must_have_explicit_return_type(definition.key.span()));
                }
                MethodDefinitionKind::Constructor | MethodDefinitionKind::Set => {}
            }
        }

        // TODO: Infer the parameter type of the `set` method from the `get` method

        let value = self.ast.function(
            FunctionType::TSEmptyBodyFunctionExpression,
            function.span,
            self.ast.copy(&function.id),
            function.generator,
            function.r#async,
            self.ast.copy(&function.this_param),
            params,
            None,
            self.ast.copy(&function.type_parameters),
            type_annotation,
            Modifiers::empty(),
        );
        self.ast.class_method(
            definition.r#type,
            definition.span,
            self.ast.copy(&definition.key),
            definition.kind,
            value,
            definition.computed,
            definition.r#static,
            definition.r#override,
            definition.optional,
            self.transform_accessibility(definition.accessibility),
            self.ast.new_vec(),
        )
    }

    pub fn create_class_property(
        &self,
        r#type: PropertyDefinitionType,
        key: PropertyKey<'a>,
        r#override: bool,
        accessibility: Option<TSAccessibility>,
    ) -> ClassElement<'a> {
        self.ast.class_property(
            r#type,
            SPAN,
            key,
            None,
            false,
            false,
            false,
            r#override,
            false,
            false,
            false,
            None,
            accessibility,
            self.ast.new_vec(),
        )
    }

    pub fn transform_formal_parameter_to_class_property(
        &self,
        param: &FormalParameter<'a>,
        type_annotation: Option<Box<'a, TSTypeAnnotation<'a>>>,
    ) -> Option<ClassElement<'a>> {
        let Some(ident_name) = param.pattern.get_identifier() else {
            // A parameter property may not be declared using a binding pattern.(1187)
            return None;
        };
        let key = self.ast.property_key_identifier(IdentifierName::new(SPAN, ident_name.clone()));
        Some(self.ast.class_property(
            PropertyDefinitionType::PropertyDefinition,
            param.span,
            key,
            None,
            false,
            false,
            false,
            param.r#override,
            param.pattern.optional,
            false,
            param.readonly,
            type_annotation,
            self.transform_accessibility(param.accessibility),
            self.ast.new_vec(),
        ))
    }

    pub fn transform_class(&self, decl: &Class<'a>) -> Option<Box<'a, Class<'a>>> {
        if decl.is_declare() {
            return None;
        }

        if let Some(super_class) = &decl.super_class {
            let is_not_allowed = match super_class {
                Expression::Identifier(_) => false,
                Expression::StaticMemberExpression(expr) => {
                    !expr.get_first_object().is_identifier_reference()
                }
                _ => true,
            };
            if is_not_allowed {
                self.error(extends_clause_expression(super_class.span()));
            }
        }

        let mut elements = self.ast.new_vec();
        let mut has_private_key = false;
        for element in &decl.body.body {
            match element {
                ClassElement::StaticBlock(_) => {}
                ClassElement::MethodDefinition(definition) => {
                    if self.report_property_key(&definition.key, definition.computed) {
                        continue;
                    }
                    if definition.key.is_private_identifier() {
                        has_private_key = true;
                        continue;
                    }

                    let function = &definition.value;
                    let params = self.transform_formal_parameters(&function.params);

                    if definition.kind.is_constructor() {
                        for (index, param) in function.params.items.iter().enumerate() {
                            if param.accessibility.is_some() {
                                // transformed params will definitely have type annotation
                                let type_annotation =
                                    self.ast.copy(&params.items[index].pattern.type_annotation);
                                if let Some(new_element) = self
                                    .transform_formal_parameter_to_class_property(
                                        param,
                                        type_annotation,
                                    )
                                {
                                    elements.push(new_element);
                                }
                            }
                        }
                    }

                    let new_element = self.transform_class_method_definition(definition, params);
                    elements.push(new_element);
                }
                ClassElement::PropertyDefinition(property) => {
                    if self.report_property_key(&property.key, property.computed) {
                        continue;
                    }

                    if property.key.is_private_identifier() {
                        has_private_key = true;
                    } else {
                        elements.push(self.transform_class_property_definition(property));
                    }
                }
                ClassElement::AccessorProperty(property) => {
                    if self.report_property_key(&property.key, property.computed) {
                        return None;
                    }

                    if property.key.is_private_identifier() {
                        has_private_key = true;
                        continue;
                    }

                    // FIXME: missing many fields
                    let new_element = self.ast.accessor_property(
                        property.r#type,
                        property.span,
                        self.ast.copy(&property.key),
                        None,
                        property.computed,
                        property.r#static,
                        self.ast.new_vec(),
                    );
                    elements.push(new_element);
                }
                ClassElement::TSIndexSignature(_) => elements.push(self.ast.copy(element)),
            }
        }

        if has_private_key {
            // <https://github.com/microsoft/TypeScript/blob/64d2eeea7b9c7f1a79edf42cb99f302535136a2e/src/compiler/transformers/declarations.ts#L1699-L1709>
            // When the class has at least one private identifier, create a unique constant identifier to retain the nominal typing behavior
            // Prevents other classes with the same public members from being used in place of the current class
            let ident = self
                .ast
                .property_key_private_identifier(PrivateIdentifier::new(SPAN, "private".into()));
            let r#type = PropertyDefinitionType::PropertyDefinition;
            let decorators = self.ast.new_vec();
            let new_element = self.ast.class_property(
                r#type, SPAN, ident, None, false, false, false, false, false, false, false, None,
                None, decorators,
            );

            elements.insert(0, new_element);
        }

        let body = self.ast.class_body(decl.body.span, elements);

        let mut modifiers = self.modifiers_declare();
        if decl.modifiers.is_contains_abstract() {
            modifiers.add_modifier(Modifier { span: SPAN, kind: ModifierKind::Abstract });
        };

        Some(self.ast.class(
            decl.r#type,
            decl.span,
            self.ast.copy(&decl.id),
            self.ast.copy(&decl.super_class),
            body,
            self.ast.copy(&decl.type_parameters),
            self.ast.copy(&decl.super_type_parameters),
            self.ast.copy(&decl.implements),
            self.ast.new_vec(),
            modifiers,
        ))
    }
}
