//! Conservative abrupt-completion analysis; no evaluation of user expressions.
use super::{Case, Stmt};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Exit {
    Normal,
    Return,
    Throw,
    Break(Option<String>),
    Continue(Option<String>),
}
type Exits = BTreeSet<Exit>;
fn one(exit: Exit) -> Exits {
    [exit].into()
}
fn sequence(body: &[Stmt]) -> Exits {
    let mut exits = one(Exit::Normal);
    for stmt in body {
        if exits.remove(&Exit::Normal) {
            exits.extend(statement(stmt));
        }
    }
    exits
}
pub(super) fn never_falls_through(body: &[Stmt]) -> bool {
    !sequence(body).contains(&Exit::Normal)
}
fn loop_exits(body: &Stmt, executes_once: bool) -> Exits {
    let mut exits = statement(body);
    if exits.remove(&Exit::Break(None)) | exits.remove(&Exit::Continue(None)) || !executes_once {
        exits.insert(Exit::Normal);
    }
    // A condition may throw even when every executed body path is abrupt.
    exits.insert(Exit::Throw);
    exits
}
fn switch_exits(cases: &[Case]) -> Exits {
    let mut exits = one(Exit::Throw); // discriminant/case evaluation can throw
    if !cases.iter().any(|case| case.test.is_none()) {
        exits.insert(Exit::Normal);
    }
    let mut suffix = one(Exit::Normal);
    for case in cases.iter().rev() {
        let mut current = sequence(&case.body);
        if current.remove(&Exit::Normal) {
            current.extend(suffix.clone());
        }
        suffix = current.clone();
        exits.extend(current);
    }
    if exits.remove(&Exit::Break(None)) {
        exits.insert(Exit::Normal);
    }
    exits
}
fn statement(stmt: &Stmt) -> Exits {
    match stmt {
        Stmt::Return(_, _) => [Exit::Return, Exit::Throw].into(),
        Stmt::Throw(_) => one(Exit::Throw),
        Stmt::Break(label) => one(Exit::Break(label.clone())),
        Stmt::Continue(label) => one(Exit::Continue(label.clone())),
        Stmt::Block(body) => sequence(body),
        Stmt::If(_, yes, no) => {
            let mut exits = statement(yes);
            exits.extend(
                no.as_ref()
                    .map_or_else(|| one(Exit::Normal), |s| statement(s)),
            );
            exits.insert(Exit::Throw);
            exits
        }
        Stmt::While(_, body) | Stmt::For(_, _, _, body) | Stmt::ForIn(_, _, _, body) => {
            loop_exits(body, false)
        }
        Stmt::DoWhile(body, _) => loop_exits(body, true),
        Stmt::Switch(_, cases) => switch_exits(cases),
        Stmt::Label(name, body) => {
            let mut exits = statement(body);
            if exits.remove(&Exit::Break(Some(name.clone())))
                | exits.remove(&Exit::Continue(Some(name.clone())))
            {
                exits.insert(Exit::Normal);
            }
            exits
        }
        Stmt::Try(body, catches, finally) => {
            let mut exits = sequence(body);
            if exits.contains(&Exit::Throw) {
                if catches
                    .iter()
                    .any(|c| c.ty.as_deref().is_none_or(|ty| ty == "*"))
                {
                    exits.remove(&Exit::Throw);
                }
                for catch in catches {
                    exits.extend(sequence(&catch.body));
                }
            }
            if let Some(finally) = finally {
                let mut final_exits = sequence(finally);
                if final_exits.remove(&Exit::Normal) {
                    final_exits.extend(exits);
                }
                final_exits
            } else {
                exits
            }
        }
        Stmt::Empty | Stmt::Function(_) => one(Exit::Normal),
        Stmt::With(_, body, _) => {
            let mut exits = statement(body);
            exits.insert(Exit::Throw);
            exits
        }
        Stmt::Expr(_) | Stmt::Var(_) => [Exit::Normal, Exit::Throw].into(),
    }
}
