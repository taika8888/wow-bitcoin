//! Breadth-first explicit-state exploration with a visited set. The model under check is the real code: a model only
//! enumerates the inputs a state can receive and applies them through the crate's public API.

use std::collections::HashSet;
use std::fmt::{self, Debug, Write as _};
use std::hash::{DefaultHasher, Hasher};
use std::time::{Duration, Instant};

pub trait Model {
    type World: Clone + Debug;
    type Step;

    fn initial(&self) -> Vec<Self::World>;
    /// Every input the world can receive next, in a fixed order, so a path replays from step indices alone.
    fn steps(&self, world: &Self::World) -> Vec<Self::Step>;
    fn describe(&self, step: &Self::Step) -> String;
    fn describe_initial(&self, world: &Self::World) -> String;
    /// The next world, and the names of the invariants this transition breaks.
    fn apply(&self, world: &Self::World, step: &Self::Step) -> (Self::World, Vec<&'static str>);
}

pub struct Report {
    pub name: &'static str,
    pub states: usize,
    pub transitions: usize,
    pub depth: usize,
    /// States discovered at the depth bound and never expanded; zero means the reachable space was exhausted.
    pub unexpanded: usize,
    pub elapsed: Duration,
    /// Each broken invariant with the shortest input sequence that breaks it, in plain words.
    pub violations: Vec<(&'static str, Vec<String>)>,
}

impl Report {
    pub fn summary(&self) -> String {
        let scope = if self.unexpanded == 0 {
            format!("exhaustive, longest path {}", self.depth)
        } else {
            format!("bounded at depth {}, {} states at the bound", self.depth, self.unexpanded)
        };
        format!(
            "model check {}: {} states, {} transitions, {scope}, {:.1}s",
            self.name,
            self.states,
            self.transitions,
            self.elapsed.as_secs_f64()
        )
    }

    pub fn failures(&self) -> String {
        let mut text = String::new();
        for (invariant, path) in &self.violations {
            let _ = writeln!(text, "invariant broken: {invariant}\n  reached by:");
            let (start, steps) = path.split_first().expect("a path starts with its initial state");
            let _ = writeln!(text, "    start: {start}");
            for (index, step) in steps.iter().enumerate() {
                let _ = writeln!(text, "    {}. {step}", index + 1);
            }
        }
        text
    }
}

/// Where a discovered state came from: its parent's index and which of the parent's steps led to it.
/// Roots carry `ROOT` and their index into `initial()`.
struct Origin {
    parent: u32,
    step: u32,
}

const ROOT: u32 = u32::MAX;

pub fn explore<M: Model>(
    name: &'static str,
    model: &M,
    max_depth: Option<usize>,
    mut visit: impl FnMut(&M::World),
) -> Report {
    let started = Instant::now();
    let mut seen = HashSet::new();
    let mut origins = Vec::new();
    let mut frontier = Vec::new();
    for (index, world) in model.initial().into_iter().enumerate() {
        if seen.insert(fingerprint(&world)) {
            visit(&world);
            frontier.push((world, origins.len() as u32));
            origins.push(Origin { parent: ROOT, step: index as u32 });
        }
    }
    let mut broken: Vec<(&'static str, u32, u32)> = Vec::new();
    let mut transitions = 0;
    let mut depth = 0;
    while !frontier.is_empty() && max_depth.is_none_or(|bound| depth < bound) {
        let mut next = Vec::new();
        for (world, id) in &frontier {
            for (step_index, step) in model.steps(world).iter().enumerate() {
                transitions += 1;
                let (after, violated) = model.apply(world, step);
                for invariant in violated {
                    if !broken.iter().any(|(name, ..)| *name == invariant) {
                        broken.push((invariant, *id, step_index as u32));
                    }
                }
                if seen.insert(fingerprint(&after)) {
                    visit(&after);
                    next.push((after, origins.len() as u32));
                    origins.push(Origin { parent: *id, step: step_index as u32 });
                }
            }
        }
        if !next.is_empty() {
            depth += 1;
        }
        frontier = next;
    }
    let violations =
        broken.into_iter().map(|(invariant, id, step)| (invariant, replay(model, &origins, id, Some(step)))).collect();
    Report {
        name,
        states: seen.len(),
        transitions,
        depth,
        unexpanded: frontier.len(),
        elapsed: started.elapsed(),
        violations,
    }
}

/// Re-runs the path to state `id` (and then `last_step`) from its root, describing the root and then each input.
fn replay<M: Model>(model: &M, origins: &[Origin], id: u32, last_step: Option<u32>) -> Vec<String> {
    let mut chain = Vec::new();
    let mut cursor = id;
    while origins[cursor as usize].parent != ROOT {
        chain.push(origins[cursor as usize].step);
        cursor = origins[cursor as usize].parent;
    }
    chain.reverse();
    chain.extend(last_step);
    let mut world = model.initial().swap_remove(origins[cursor as usize].step as usize);
    let mut described = vec![model.describe_initial(&world)];
    for step_index in chain {
        let step = model.steps(&world).swap_remove(step_index as usize);
        described.push(model.describe(&step));
        world = model.apply(&world, &step).0;
    }
    described
}

/// 128 bits of two SipHash streams over the world's `Debug` form. `Debug` prints every field, private ones included,
/// and prints floats in their shortest exact form, so two worlds share a form only when they are equal. Hashing the
/// form keeps memory flat; at a million states the chance that two different worlds collide is below 1e-26.
fn fingerprint(world: &impl Debug) -> u128 {
    struct Streams(DefaultHasher, DefaultHasher);
    impl fmt::Write for Streams {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            self.0.write(text.as_bytes());
            self.1.write(text.as_bytes());
            Ok(())
        }
    }
    let mut streams = Streams(DefaultHasher::new(), DefaultHasher::new());
    streams.1.write_u8(0xa5);
    let _ = write!(streams, "{world:?}");
    (u128::from(streams.0.finish()) << 64) | u128::from(streams.1.finish())
}
