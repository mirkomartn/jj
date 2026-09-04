use hegel::generators as gs;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Revset {
    Base(String),
    SingleArg(String, Box<Self>),
    SingleArgWithDepth(String, Box<Self>, u32),
    AtOp(String, Box<Self>),
}

impl std::fmt::Display for Revset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Base(name) => write!(f, "{name}"),
            Self::SingleArg(func, arg) => write!(f, "{func}({arg})"),
            Self::SingleArgWithDepth(func, arg, depth) => {
                write!(f, "{func}({arg}, {depth})")
            }
            Self::AtOp(op, inner) => write!(f, "at_operation({op}, {inner})"),
        }
    }
}

pub(crate) fn draw_revset(tc: &hegel::TestCase, max_depth: u32, ops: &[String]) -> String {
    tc.draw(draw_revset_inner(max_depth, ops)).to_string()
}

#[hegel::composite]
fn draw_revset_inner(tc: &hegel::TestCase, max_depth: u32, ops: &[String]) -> Revset {
    tc.assume(!ops.is_empty());

    let no_args = [
        "@",
        "all()",
        "conflicts()",
        "divergent()",
        "forks()",
        "merges()",
        "root()",
        "visible_heads()",
    ];

    let without_depth = ["bisect", "connected", "heads", "merge_point", "roots"];

    let with_depth = [
        "ancestors",
        "children",
        "descendants",
        "first_ancestors",
        "first_parent",
        "latest",
        "parents",
    ];

    // Choose random function type.
    let func = tc.draw(hegel::one_of!(
        gs::sampled_from(&no_args),
        gs::sampled_from(&with_depth),
        gs::sampled_from(&without_depth),
        gs::just("at_operation")
    ));

    // Base case: return a no_args function.
    if no_args.contains(&func) || max_depth == 0 {
        let choice = tc.draw(gs::integers::<usize>().max_value(no_args.len() - 1));
        return Revset::Base(no_args[choice].to_string());
    }

    let inner = tc.draw(draw_revset_inner(max_depth - 1, ops));

    if func == "at_operation" {
        let op = tc.draw(gs::sampled_from(ops));
        return Revset::AtOp(op, Box::new(inner));
    } else if without_depth.contains(&func) {
        return Revset::SingleArg(func.to_string(), Box::new(inner));
    } else {
        let depth = tc.draw(gs::integers::<u32>());
        return Revset::SingleArgWithDepth(func.to_string(), Box::new(inner), depth);
    }
}
