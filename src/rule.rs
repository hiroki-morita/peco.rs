use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::{Rc, Weak},
};

use crate::{GrammarError, Parser, error::Failure};

pub(crate) struct Grammar {
    ready: Cell<bool>,
    rules: RefCell<Vec<Rc<dyn ErasedRule>>>,
}

#[derive(Clone)]
pub(crate) struct Dependency {
    pub(crate) owner: Weak<Grammar>,
    name: String,
}

trait ErasedRule {
    fn validate(&self, owner: &Rc<Grammar>) -> Result<(), GrammarError>;
}

struct RuleCell<T> {
    name: String,
    body: OnceCell<Parser<T>>,
}

impl<T: 'static> ErasedRule for RuleCell<T> {
    fn validate(&self, owner: &Rc<Grammar>) -> Result<(), GrammarError> {
        let body = self.body.get().ok_or_else(|| GrammarError::UndefinedRule {
            rule: self.name.clone(),
        })?;
        validate_dependencies(&body.dependencies, owner)
    }
}

fn validate_dependencies(
    dependencies: &[Dependency],
    owner: &Rc<Grammar>,
) -> Result<(), GrammarError> {
    for dependency in dependencies {
        if !dependency.owner.ptr_eq(&Rc::downgrade(owner)) {
            return Err(GrammarError::ForeignRule {
                rule: dependency.name.clone(),
            });
        }
    }
    Ok(())
}

/// A typed, non-owning handle to a declared nonterminal.
/// Cloning a handle does not keep a grammar alive or require `T: Clone`.
pub struct Rule<T> {
    cell: Weak<RuleCell<T>>,
    owner: Weak<Grammar>,
    name: String,
}

impl<T> Clone for Rule<T> {
    fn clone(&self) -> Self {
        Self {
            cell: self.cell.clone(),
            owner: self.owner.clone(),
            name: self.name.clone(),
        }
    }
}

/// Declares and defines mutually recursive rules before creating an owning root.
/// Unfinished references can be composed but cannot be executed successfully.
pub struct GrammarBuilder {
    owner: Rc<Grammar>,
}

impl Default for GrammarBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GrammarBuilder {
    /// Creates an empty grammar builder.
    pub fn new() -> Self {
        Self {
            owner: Rc::new(Grammar {
                ready: Cell::new(false),
                rules: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Declares a rule with an output type. Names are diagnostic labels, not IDs.
    pub fn declare<T: 'static>(&mut self, name: impl Into<String>) -> Rule<T> {
        let cell = Rc::new(RuleCell {
            name: name.into(),
            body: OnceCell::new(),
        });
        let rule = Rule {
            cell: Rc::downgrade(&cell),
            owner: Rc::downgrade(&self.owner),
            name: cell.name.clone(),
        };
        self.owner.rules.borrow_mut().push(cell);
        rule
    }

    /// Creates a non-owning reference to a rule from this builder.
    pub fn reference<T: 'static>(&self, rule: &Rule<T>) -> Result<Parser<T>, GrammarError> {
        self.check(rule)?;
        let rule = rule.clone();
        let dependency = Dependency {
            owner: rule.owner.clone(),
            name: rule.name.clone(),
        };
        let mut parser = Parser::new(move |ctx, pos| {
            let owner = rule.owner.upgrade().ok_or_else(|| {
                Failure::Fatal(GrammarError::ExpiredRule {
                    rule: rule.name.clone(),
                })
            })?;
            if !owner.ready.get() {
                return Err(Failure::Fatal(GrammarError::UndefinedRule {
                    rule: rule.name.clone(),
                }));
            }
            let cell = rule.cell.upgrade().ok_or_else(|| {
                Failure::Fatal(GrammarError::ExpiredRule {
                    rule: rule.name.clone(),
                })
            })?;
            let body = cell.body.get().ok_or_else(|| {
                Failure::Fatal(GrammarError::UndefinedRule {
                    rule: rule.name.clone(),
                })
            })?;
            body.run(ctx, pos)
        });
        parser.dependencies.push(dependency);
        Ok(parser)
    }

    /// Sets a rule's body once. References in the body are validated by `finish`.
    pub fn define<T: 'static>(
        &mut self,
        rule: &Rule<T>,
        body: Parser<T>,
    ) -> Result<(), GrammarError> {
        self.check(rule)?;
        let cell = rule
            .cell
            .upgrade()
            .ok_or_else(|| GrammarError::ExpiredRule {
                rule: rule.name.clone(),
            })?;
        cell.body
            .set(body)
            .map_err(|_| GrammarError::DuplicateDefinition {
                rule: rule.name.clone(),
            })
    }

    fn check<T>(&self, rule: &Rule<T>) -> Result<(), GrammarError> {
        if rule.owner.ptr_eq(&Rc::downgrade(&self.owner)) {
            Ok(())
        } else {
            Err(GrammarError::ForeignRule {
                rule: rule.name.clone(),
            })
        }
    }

    /// Validates every declaration and returns a root that keeps the grammar alive.
    /// An error consumes the builder; all of its unfinished references then expire.
    pub fn finish<T: 'static>(self, root: Parser<T>) -> Result<Parser<T>, GrammarError> {
        validate_dependencies(&root.dependencies, &self.owner)?;
        for rule in self.owner.rules.borrow().iter() {
            rule.validate(&self.owner)?;
        }
        self.owner.ready.set(true);
        let mut result = root.wrap(move |root, ctx, pos| {
            let _keep_alive = &self.owner;
            root.run(ctx, pos)
        });
        result.dependencies.clear();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, literal};

    #[test]
    fn validates_definitions_and_foreign_references() {
        let mut builder = GrammarBuilder::new();
        let rule = builder.declare::<()>("A");
        let reference = builder.reference(&rule).unwrap();
        assert!(matches!(
            reference.parse(""),
            Err(ParseError::Fatal(GrammarError::UndefinedRule { .. }))
        ));
        assert!(matches!(
            builder.finish(reference),
            Err(GrammarError::UndefinedRule { .. })
        ));
        let mut builder = GrammarBuilder::new();
        let rule = builder.declare("A");
        builder.define(&rule, literal("a")).unwrap();
        assert!(matches!(
            builder.define(&rule, literal("b")),
            Err(GrammarError::DuplicateDefinition { .. })
        ));
        let mut other = GrammarBuilder::new();
        assert!(matches!(
            other.reference(&rule),
            Err(GrammarError::ForeignRule { .. })
        ));
        assert!(matches!(
            other.define(&rule, literal("")),
            Err(GrammarError::ForeignRule { .. })
        ));
        let foreign = builder.reference(&rule).unwrap().optional().ignore();
        assert!(matches!(
            other.finish(foreign),
            Err(GrammarError::ForeignRule { .. })
        ));
    }

    #[test]
    fn validates_foreign_references_inside_bodies() {
        let mut first = GrammarBuilder::new();
        let foreign = first.declare("foreign");
        first.define(&foreign, literal("a")).unwrap();
        let mut second = GrammarBuilder::new();
        let local = second.declare("local");
        second
            .define(
                &local,
                literal("")
                    .ignore_then(first.reference(&foreign).unwrap())
                    .map(|_| ()),
            )
            .unwrap();
        assert!(matches!(
            second.finish(literal("")),
            Err(GrammarError::ForeignRule { .. })
        ));
    }

    #[test]
    fn roots_keep_heterogeneous_rules_alive_without_cycles() {
        let mut builder = GrammarBuilder::new();
        let a = builder.declare("A");
        let b = builder.declare("B");
        builder.define(&a, literal("a").map(|_| 42_u32)).unwrap();
        builder.define(&b, literal("b").text()).unwrap();
        let raw = builder.reference(&a).unwrap();
        let root = raw.clone().then(builder.reference(&b).unwrap());
        let owner = Rc::downgrade(&builder.owner);
        let cell = a.cell.clone();
        let root = builder.finish(root).unwrap();
        let clone = root.clone().then_ignore(literal("!"));
        drop(root);
        assert_eq!(clone.parse("ab!"), Ok((42, "b".into())));
        drop(clone);
        assert!(owner.upgrade().is_none());
        assert!(cell.upgrade().is_none());
        assert!(matches!(
            raw.parse("a"),
            Err(ParseError::Fatal(GrammarError::ExpiredRule { .. }))
        ));
    }
}
