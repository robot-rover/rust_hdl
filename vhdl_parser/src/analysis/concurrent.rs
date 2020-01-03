// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this file,
// You can obtain one at http://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2019, Olof Kraigher olof.kraigher@gmail.com

use super::*;
use crate::ast::*;
use crate::data::*;
use analyze::*;
use region::*;

impl<'a> AnalyzeContext<'a> {
    pub fn analyze_concurrent_part(
        &self,
        parent: &mut Region<'_>,
        statements: &mut [LabeledConcurrentStatement],
        diagnostics: &mut dyn DiagnosticHandler,
    ) -> FatalNullResult {
        for statement in statements.iter_mut() {
            self.analyze_concurrent_statement(parent, statement, diagnostics)?;
        }

        Ok(())
    }

    fn analyze_concurrent_statement(
        &self,
        parent: &mut Region<'_>,
        statement: &mut LabeledConcurrentStatement,
        diagnostics: &mut dyn DiagnosticHandler,
    ) -> FatalNullResult {
        if let Some(ref label) = statement.label {
            parent.add(label.clone(), NamedEntityKind::Constant, diagnostics);
        }

        match statement.statement {
            ConcurrentStatement::Block(ref mut block) => {
                let mut region = parent.nested();
                self.analyze_declarative_part(&mut region, &mut block.decl, diagnostics)?;
                self.analyze_concurrent_part(&mut region, &mut block.statements, diagnostics)?;
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
                                self.resolve_name(parent, &name.pos, &mut name.item, diagnostics)?;
                            }
                        }
                        SensitivityList::All => {}
                    }
                }
                let mut region = parent.nested();
                self.analyze_declarative_part(&mut region, decl, diagnostics)?;
                self.analyze_sequential_part(&mut region, statements, diagnostics)?;
            }
            ConcurrentStatement::ForGenerate(ref mut gen) => {
                let ForGenerateStatement {
                    index_name,
                    discrete_range,
                    body,
                } = gen;
                self.analyze_discrete_range(parent, discrete_range, diagnostics)?;
                let mut region = parent.nested();
                region.add(index_name.clone(), NamedEntityKind::Constant, diagnostics);
                self.analyze_generate_body(&mut region, body, diagnostics)?;
            }
            ConcurrentStatement::IfGenerate(ref mut gen) => {
                let Conditionals {
                    conditionals,
                    else_item,
                } = gen;
                for conditional in conditionals.iter_mut() {
                    let Conditional { condition, item } = conditional;
                    self.analyze_expression(parent, condition, diagnostics)?;
                    let mut region = parent.nested();
                    self.analyze_generate_body(&mut region, item, diagnostics)?;
                }
                if let Some(ref mut else_item) = else_item {
                    let mut region = parent.nested();
                    self.analyze_generate_body(&mut region, else_item, diagnostics)?;
                }
            }
            ConcurrentStatement::CaseGenerate(ref mut gen) => {
                for alternative in gen.alternatives.iter_mut() {
                    let mut region = parent.nested();
                    self.analyze_generate_body(&mut region, &mut alternative.item, diagnostics)?;
                }
            }
            ConcurrentStatement::Instance(ref mut instance) => {
                self.analyze_instance(parent, instance, diagnostics)?;
            }
            ConcurrentStatement::Assignment(ref mut assign) => {
                // @TODO more delaymechanism
                let ConcurrentSignalAssignment { target, rhs, .. } = assign;
                self.analyze_waveform_assignment(parent, target, rhs, diagnostics)?;
            }
            ConcurrentStatement::ProcedureCall(ref mut pcall) => {
                let ConcurrentProcedureCall {
                    call,
                    postponed: _postponed,
                } = pcall;
                self.analyze_function_call(parent, call, diagnostics)?;
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
                self.analyze_expression(parent, condition, diagnostics)?;
                if let Some(expr) = report {
                    self.analyze_expression(parent, expr, diagnostics)?;
                }
                if let Some(expr) = severity {
                    self.analyze_expression(parent, expr, diagnostics)?;
                }
            }
        };
        Ok(())
    }

    fn analyze_generate_body(
        &self,
        region: &mut Region<'_>,
        body: &mut GenerateBody,
        diagnostics: &mut dyn DiagnosticHandler,
    ) -> FatalNullResult {
        let GenerateBody {
            alternative_label,
            decl,
            statements,
        } = body;
        if let Some(label) = alternative_label {
            region.add(label.clone(), NamedEntityKind::Constant, diagnostics);
        }
        if let Some(ref mut decl) = decl {
            self.analyze_declarative_part(region, decl, diagnostics)?;
        }
        self.analyze_concurrent_part(region, statements, diagnostics)?;

        Ok(())
    }

    fn analyze_instance(
        &self,
        parent: &Region<'_>,
        instance: &mut InstantiationStatement,
        diagnostics: &mut dyn DiagnosticHandler,
    ) -> FatalNullResult {
        match instance.unit {
            // @TODO architecture
            InstantiatedUnit::Entity(ref mut entity_name, ..) => {
                if let Err(err) = self.resolve_selected_name(parent, entity_name) {
                    err.add_to(diagnostics)?;
                }
            }
            InstantiatedUnit::Component(ref mut component_name) => {
                if let Err(err) = self.resolve_selected_name(parent, component_name) {
                    err.add_to(diagnostics)?;
                }
            }
            InstantiatedUnit::Configuration(ref mut config_name) => {
                if let Err(err) = self.resolve_selected_name(parent, config_name) {
                    err.add_to(diagnostics)?;
                }
            }
        };

        self.analyze_assoc_elems(parent, &mut instance.generic_map, diagnostics)?;
        self.analyze_assoc_elems(parent, &mut instance.port_map, diagnostics)?;

        Ok(())
    }
}
