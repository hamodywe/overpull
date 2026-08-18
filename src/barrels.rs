//! `overpull barrels` — re-export files and what they amplify.
//!
//! A barrel is a module whose exports are predominantly re-exports. Present
//! tooling flags their *existence*; what costs build and test time is their
//! *amplification*: importing the barrel loads every target it fans out to,
//! while the importer typically needed one. Amplification is measured, not
//! assumed: full load cost of the barrel versus the median load cost of its
//! direct re-export targets.

use std::collections::BTreeSet;

use crate::graph::ModuleGraph;

pub struct BarrelReport {
    pub module: usize,
    /// Named re-exports plus `export *` statements (runtime only).
    pub reexport_count: usize,
    pub star_count: usize,
    pub local_export_count: usize,
    /// Modules loaded by importing this barrel (including itself).
    pub cost_modules: usize,
    pub cost_bytes: u64,
    /// Median load cost among the barrel's direct re-export targets — what a
    /// direct import would have cost instead.
    pub median_target_cost: usize,
    /// `cost_modules / max(median_target_cost, 1)`, rounded to one decimal.
    pub amplification: f64,
    pub importer_count: usize,
    pub external_packages: usize,
}

pub struct Thresholds {
    /// Minimum runtime re-exports for a file to count as a barrel.
    pub min_reexports: usize,
    /// Minimum barrel load cost (modules) to be worth reporting.
    pub min_cost: usize,
    /// Minimum amplification factor to be worth reporting.
    pub min_amplification: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            min_reexports: 3,
            min_cost: 20,
            min_amplification: 4.0,
        }
    }
}

pub fn analyze(graph: &ModuleGraph, thresholds: &Thresholds) -> Vec<BarrelReport> {
    let mut reports = Vec::new();
    for (idx, module) in graph.modules.iter().enumerate() {
        let named = module
            .facts
            .named_reexports
            .iter()
            .filter(|r| !r.is_type)
            .count();
        let stars = module
            .facts
            .star_reexports
            .iter()
            .filter(|r| !r.is_type)
            .count();
        let reexport_count = named + stars;
        let local = module.facts.local_value_export_count;
        if reexport_count < thresholds.min_reexports {
            continue;
        }
        // Predominantly re-exports: local declarations are at most a fifth of
        // the export surface. A module that mostly declares and re-exports a
        // little is an implementation file, not a barrel.
        if local * 4 > reexport_count {
            continue;
        }

        let cost = graph.load_cost(idx);
        if cost.modules.len() < thresholds.min_cost {
            continue;
        }

        // Direct re-export targets, through the resolved edges.
        let mut target_specs: BTreeSet<&str> = BTreeSet::new();
        for r in &module.facts.named_reexports {
            if !r.is_type {
                target_specs.insert(r.specifier.as_str());
            }
        }
        for r in &module.facts.star_reexports {
            if !r.is_type {
                target_specs.insert(r.specifier.as_str());
            }
        }
        let mut target_costs: Vec<usize> = module
            .edges
            .iter()
            .filter(|e| target_specs.contains(e.specifier.as_str()))
            .map(|e| graph.load_cost(e.to).modules.len())
            .collect();
        target_costs.sort_unstable();
        let median_target_cost = if target_costs.is_empty() {
            1
        } else {
            target_costs[target_costs.len() / 2]
        };

        #[allow(clippy::cast_precision_loss)]
        let amplification = cost.modules.len() as f64 / median_target_cost.max(1) as f64;
        if amplification < thresholds.min_amplification {
            continue;
        }

        reports.push(BarrelReport {
            module: idx,
            reexport_count,
            star_count: stars,
            local_export_count: local,
            cost_modules: cost.modules.len(),
            cost_bytes: cost.total_bytes,
            median_target_cost,
            amplification: (amplification * 10.0).round() / 10.0,
            importer_count: graph.importers[idx].len(),
            external_packages: cost.external_packages.len(),
        });
    }
    reports.sort_by(|a, b| {
        // Cost to the codebase is amplification felt by every importer.
        let pain_a = a.cost_modules * a.importer_count.max(1);
        let pain_b = b.cost_modules * b.importer_count.max(1);
        pain_b.cmp(&pain_a).then_with(|| {
            graph
                .display_path(a.module)
                .cmp(&graph.display_path(b.module))
        })
    });
    reports
}
