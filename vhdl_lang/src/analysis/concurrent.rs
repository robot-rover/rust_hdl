// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this file,
// You can obtain one at http://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2019, Olof Kraigher olof.kraigher@gmail.com

// These fields are better explicit than .. since we are forced to consider if new fields should be searched
#![allow(clippy::unneeded_field_pattern)]

use super::*;
use crate::ast::*;
use crate::data::*;
use analyze::*;
use region::*;
use target::AssignmentType;

impl<'a> AnalyzeContext<'a> {
    pub fn analyze_concurrent_part(
        &self,
        scope: &mut Scope<'_>,
        statements: &mut [LabeledConcurrentStatement],
        diagnostics: &mut dyn DiagnosticHandler,
    ) -> FatalNullResult {
        for statement in statements.iter_mut() {
            self.analyze_concurrent_statement(scope, statement, diagnostics)?;
        }

        Ok(())
    }

    fn analyze_concurrent_statement(
        &self,
        scope: &mut Scope<'_>,
        statement: &mut LabeledConcurrentStatement,
        diagnostics: &mut dyn DiagnosticHandler,
    ) -> FatalNullResult {
        if let Some(ref mut label) = statement.label {
            scope.add(label.define(NamedEntityKind::Label), diagnostics);
        }

        match statement.statement {
            ConcurrentStatement::Block(ref mut block) => {
                if let Some(ref mut guard_condition) = block.guard_condition {
                    self.analyze_expression(scope, guard_condition, diagnostics)?;
                }
                let mut nested = scope.nested();
                if let Some(ref mut list) = block.header.generic_clause {
                    self.analyze_interface_list(&mut nested, list, diagnostics)?;
                }
                if let Some(ref mut list) = block.header.generic_map {
                    self.analyze_assoc_elems(scope, list, diagnostics)?;
                }
                if let Some(ref mut list) = block.header.port_clause {
                    self.analyze_interface_list(&mut nested, list, diagnostics)?;
                }
                if let Some(ref mut list) = block.header.port_map {
                    self.analyze_assoc_elems(scope, list, diagnostics)?;
                }
                self.analyze_declarative_part(&mut nested, &mut block.decl, diagnostics)?;
                self.analyze_concurrent_part(&mut nested, &mut block.statements, diagnostics)?;
            }
            ConcurrentStatement::Process(ref mut process) => {
                let ProcessStatement {
                    postponed: _,
                    sensitivity_list,
                    decl,
                    statements,
                } = process;
                if let Some(sensitivity_list) = sensitivity_list {
                    match sensitivity_list {
                        SensitivityList::Names(names) => {
                            for name in names.iter_mut() {
                                self.resolve_name(scope, &name.pos, &mut name.item, diagnostics)?;
                            }
                        }
                        SensitivityList::All => {}
                    }
                }
                let mut nested = scope.nested();
                self.analyze_declarative_part(&mut nested, decl, diagnostics)?;
                self.analyze_sequential_part(&mut nested, statements, diagnostics)?;
            }
            ConcurrentStatement::ForGenerate(ref mut gen) => {
                let ForGenerateStatement {
                    index_name,
                    discrete_range,
                    body,
                } = gen;
                self.analyze_discrete_range(scope, discrete_range, diagnostics)?;
                let mut nested = scope.nested();
                nested.add(
                    index_name.define(NamedEntityKind::LoopParameter),
                    diagnostics,
                );
                self.analyze_generate_body(&mut nested, body, diagnostics)?;
            }
            ConcurrentStatement::IfGenerate(ref mut gen) => {
                let Conditionals {
                    conditionals,
                    else_item,
                } = gen;
                for conditional in conditionals.iter_mut() {
                    let Conditional { condition, item } = conditional;
                    self.analyze_expression(scope, condition, diagnostics)?;
                    let mut nested = scope.nested();
                    self.analyze_generate_body(&mut nested, item, diagnostics)?;
                }
                if let Some(ref mut else_item) = else_item {
                    let mut nested = scope.nested();
                    self.analyze_generate_body(&mut nested, else_item, diagnostics)?;
                }
            }
            ConcurrentStatement::CaseGenerate(ref mut gen) => {
                for alternative in gen.alternatives.iter_mut() {
                    let mut nested = scope.nested();
                    self.analyze_generate_body(&mut nested, &mut alternative.item, diagnostics)?;
                }
            }
            ConcurrentStatement::Instance(ref mut instance) => {
                self.analyze_instance(scope, instance, diagnostics)?;
            }
            ConcurrentStatement::Assignment(ref mut assign) => {
                // @TODO more delaymechanism
                let ConcurrentSignalAssignment { target, rhs, .. } = assign;
                self.analyze_waveform_assignment(
                    scope,
                    target,
                    AssignmentType::Signal,
                    rhs,
                    diagnostics,
                )?;
            }
            ConcurrentStatement::ProcedureCall(ref mut pcall) => {
                let ConcurrentProcedureCall { call, .. } = pcall;
                self.analyze_procedure_call(scope, call, diagnostics)?;
            }
            ConcurrentStatement::Assert(ref mut assert) => {
                let ConcurrentAssertStatement {
                    postponed: _postponed,
                    statement:
                        AssertStatement {
                            condition,
                            report,
                            severity,
                        },
                } = assert;
                self.analyze_expression(scope, condition, diagnostics)?;
                if let Some(expr) = report {
                    self.analyze_expression(scope, expr, diagnostics)?;
                }
                if let Some(expr) = severity {
                    self.analyze_expression(scope, expr, diagnostics)?;
                }
            }
        };
        Ok(())
    }

    fn analyze_generate_body(
        &self,
        scope: &mut Scope<'_>,
        body: &mut GenerateBody,
        diagnostics: &mut dyn DiagnosticHandler,
    ) -> FatalNullResult {
        let GenerateBody {
            alternative_label,
            decl,
            statements,
        } = body;
        if let Some(label) = alternative_label {
            scope.add(label.define(NamedEntityKind::Label), diagnostics);
        }
        if let Some(ref mut decl) = decl {
            self.analyze_declarative_part(scope, decl, diagnostics)?;
        }
        self.analyze_concurrent_part(scope, statements, diagnostics)?;

        Ok(())
    }

    fn analyze_instance(
        &self,
        scope: &Scope<'_>,
        instance: &mut InstantiationStatement,
        diagnostics: &mut dyn DiagnosticHandler,
    ) -> FatalNullResult {
        match instance.unit {
            // @TODO architecture
            InstantiatedUnit::Entity(ref mut entity_name, ..) => {
                if let Err(err) =
                    self.resolve_selected_name(scope, entity_name)
                        .and_then(|entities| {
                            let expected = "entity";
                            let ent = self.resolve_non_overloaded(
                                entities,
                                entity_name.suffix_pos(),
                                expected,
                            )?;

                            if let NamedEntityKind::Entity(ent_region) = ent.kind() {
                                let (generic_region, port_region) = ent_region.to_entity_formal();

                                self.analyze_assoc_elems_with_formal_region(
                                    &entity_name.pos,
                                    &generic_region,
                                    scope,
                                    &mut instance.generic_map,
                                    diagnostics,
                                )?;
                                self.analyze_assoc_elems_with_formal_region(
                                    &entity_name.pos,
                                    &port_region,
                                    scope,
                                    &mut instance.port_map,
                                    diagnostics,
                                )?;
                                Ok(())
                            } else {
                                Err(AnalysisError::NotFatal(
                                    ent.kind_error(entity_name.suffix_pos(), expected),
                                ))
                            }
                        })
                {
                    err.add_to(diagnostics)?;
                }
            }
            InstantiatedUnit::Component(ref mut component_name) => {
                if let Err(err) =
                    self.resolve_selected_name(scope, component_name)
                        .and_then(|entities| {
                            let expected = "component";
                            let ent = self.resolve_non_overloaded(
                                entities,
                                component_name.suffix_pos(),
                                expected,
                            )?;

                            if let NamedEntityKind::Component(ent_region) = ent.kind() {
                                let (generic_region, port_region) = ent_region.to_entity_formal();
                                self.analyze_assoc_elems_with_formal_region(
                                    &component_name.pos,
                                    &generic_region,
                                    scope,
                                    &mut instance.generic_map,
                                    diagnostics,
                                )?;
                                self.analyze_assoc_elems_with_formal_region(
                                    &component_name.pos,
                                    &port_region,
                                    scope,
                                    &mut instance.port_map,
                                    diagnostics,
                                )?;
                                Ok(())
                            } else {
                                Err(AnalysisError::NotFatal(
                                    ent.kind_error(component_name.suffix_pos(), expected),
                                ))
                            }
                        })
                {
                    err.add_to(diagnostics)?;
                }
            }
            InstantiatedUnit::Configuration(ref mut config_name) => {
                fn is_configuration(kind: &NamedEntityKind) -> bool {
                    matches!(kind, NamedEntityKind::Configuration(..))
                }

                if let Err(err) =
                    self.resolve_selected_name(scope, config_name)
                        .and_then(|entities| {
                            self.resolve_non_overloaded_with_kind(
                                entities,
                                config_name.suffix_pos(),
                                &is_configuration,
                                "configuration",
                            )
                        })
                {
                    err.add_to(diagnostics)?;
                }

                self.analyze_assoc_elems(scope, &mut instance.generic_map, diagnostics)?;
                self.analyze_assoc_elems(scope, &mut instance.port_map, diagnostics)?;
            }
        };

        Ok(())
    }
}
