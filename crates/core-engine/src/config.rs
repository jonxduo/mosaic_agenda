//! A configuration version with lookups.

use std::collections::BTreeMap;
use std::sync::Arc;

use core_config::CompiledConfig;
use core_model::Slug;
use core_model::config::{
    AggregateDef, FormDef, ProgramTypeDef, QualificationDef, ResourceTypeDef, RoleDef, StepDef,
    UnitTypeDef, WorkflowDef,
};
use core_model::eval::ConfigIndex;

#[derive(Debug, Clone)]
pub struct Cfg {
    pub version: u32,
    pub compiled: Arc<CompiledConfig>,
}

impl Cfg {
    pub fn new(version: u32, compiled: CompiledConfig) -> Self {
        Self {
            version,
            compiled: Arc::new(compiled),
        }
    }

    pub fn index(&self) -> &ConfigIndex {
        &self.compiled.index
    }

    fn p(&self) -> &core_model::config::PackageConfig {
        &self.compiled.package
    }

    pub fn qualification(&self, id: &Slug) -> Option<&QualificationDef> {
        self.p()
            .qualifications
            .qualifications
            .iter()
            .find(|q| &q.id == id)
    }

    pub fn unit_type(&self, id: &Slug) -> Option<&UnitTypeDef> {
        self.p().units.unit_types.iter().find(|u| &u.id == id)
    }

    pub fn role(&self, id: &Slug) -> Option<&RoleDef> {
        self.p().roles.roles.iter().find(|r| &r.id == id)
    }

    pub fn roles(&self) -> &[RoleDef] {
        &self.p().roles.roles
    }

    pub fn form(&self, id: &Slug) -> Option<&FormDef> {
        self.p().forms.forms.iter().find(|f| &f.id == id)
    }

    pub fn forms(&self) -> &[FormDef] {
        &self.p().forms.forms
    }

    pub fn workflow(&self, id: &Slug) -> Option<&WorkflowDef> {
        self.p().workflows.workflows.iter().find(|w| &w.id == id)
    }

    pub fn aggregates(&self) -> &[AggregateDef] {
        &self.p().aggregates.aggregates
    }

    pub fn aggregate(&self, id: &Slug) -> Option<&AggregateDef> {
        self.p().aggregates.aggregates.iter().find(|a| &a.id == id)
    }

    pub fn program_type(&self, id: &Slug) -> Option<&ProgramTypeDef> {
        self.p().programs.program_types.iter().find(|t| &t.id == id)
    }

    pub fn resource_type(&self, id: &Slug) -> Option<&ResourceTypeDef> {
        self.p()
            .resources
            .resource_types
            .iter()
            .find(|t| &t.id == id)
    }

    pub fn package(&self) -> &core_model::config::PackageConfig {
        self.p()
    }

    /// Workflow of a form.
    pub fn form_workflow(&self, form: &FormDef) -> Option<&WorkflowDef> {
        self.workflow(&form.workflow)
    }

    /// Read audience of a form in a state: the state's entry, else the
    /// nearest earlier state's, else the subject only.
    pub fn read_audience(&self, form: &FormDef, state: &Slug) -> Slug {
        if let Some(a) = form.read_audience_by_state.get(state) {
            return a.clone();
        }
        if let Some(w) = self.form_workflow(form) {
            if let Some(pos) = w.states.iter().position(|s| s == state) {
                for s in w.states[..pos].iter().rev() {
                    if let Some(a) = form.read_audience_by_state.get(s) {
                        return a.clone();
                    }
                }
            }
        }
        Slug::lit("subject_only")
    }

    /// Position of a state in its workflow (for "at least" comparisons).
    pub fn state_rank(&self, form: &FormDef, state: &Slug) -> Option<usize> {
        self.form_workflow(form)?
            .states
            .iter()
            .position(|s| s == state)
    }

    pub fn step<'a>(&self, workflow: &'a WorkflowDef, id: &Slug) -> Option<&'a StepDef> {
        workflow.steps.iter().find(|s| &s.id == id)
    }
}

/// Configuration versions known to the projection.
#[derive(Debug, Clone, Default)]
pub struct Configs {
    pub versions: BTreeMap<u32, Cfg>,
}

impl Configs {
    pub fn get(&self, version: u32) -> Option<&Cfg> {
        self.versions.get(&version)
    }

    pub fn latest(&self) -> Option<&Cfg> {
        self.versions.values().next_back()
    }
}
