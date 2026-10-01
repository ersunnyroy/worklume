//! What the member may track (E09-S09): projects with their tasks, and the organization's break
//! policies.
//!
//! The server sends the catalog (`GET /v1/desktop/projects` and the configuration, filled in by
//! E09-S06). Every timer request from the agent's own UI names its project, task or break
//! policy by ID, and the agent checks that ID against the catalog before acting on it: the
//! webview is a client like any other, and its IDs are never trusted. The server checks again
//! when the events arrive (`project_unavailable`).

use uuid::Uuid;

use crate::tracker::WorkTarget;

/// The member's trackable projects and the organization's break policies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub projects: Vec<Project>,
    pub break_policies: Vec<BreakPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakPolicy {
    pub id: Uuid,
    pub name: String,
}

/// An ID the catalog doesn't hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CatalogError {
    #[error("the project is not one the member may track")]
    UnknownProject,
    #[error("the task does not belong to the project")]
    UnknownTask,
    #[error("the break policy is not one of the organization's")]
    UnknownBreakPolicy,
}

impl Catalog {
    /// The work target for `project_id` and `task_id`, if the member may track it.
    ///
    /// # Errors
    ///
    /// Fails when the project isn't in the catalog, or the task isn't one of its tasks.
    pub fn target(
        &self,
        project_id: Uuid,
        task_id: Option<Uuid>,
    ) -> Result<WorkTarget, CatalogError> {
        let project = self
            .project(project_id)
            .ok_or(CatalogError::UnknownProject)?;
        if let Some(task_id) = task_id
            && !project.tasks.iter().any(|task| task.id == task_id)
        {
            return Err(CatalogError::UnknownTask);
        }
        Ok(WorkTarget {
            project_id,
            task_id,
        })
    }

    /// The project for idle time to be reassigned to, if the member may track it.
    ///
    /// # Errors
    ///
    /// Fails when the project isn't in the catalog.
    pub fn reassign_target(&self, project_id: Uuid) -> Result<Uuid, CatalogError> {
        self.project(project_id)
            .map(|project| project.id)
            .ok_or(CatalogError::UnknownProject)
    }

    /// The break policy with `id`.
    ///
    /// # Errors
    ///
    /// Fails when the organization has no such policy.
    pub fn break_policy(&self, id: Uuid) -> Result<&BreakPolicy, CatalogError> {
        self.break_policies
            .iter()
            .find(|policy| policy.id == id)
            .ok_or(CatalogError::UnknownBreakPolicy)
    }

    #[must_use]
    pub fn project(&self, id: Uuid) -> Option<&Project> {
        self.projects.iter().find(|project| project.id == id)
    }
}

impl Project {
    #[must_use]
    pub fn task(&self, id: Uuid) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{BreakPolicy, Catalog, CatalogError, Project, Task};

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn catalog() -> Catalog {
        Catalog {
            projects: vec![
                Project {
                    id: id(1),
                    name: "Website".to_owned(),
                    tasks: vec![Task {
                        id: id(11),
                        name: "Checkout".to_owned(),
                    }],
                },
                Project {
                    id: id(2),
                    name: "Support".to_owned(),
                    tasks: Vec::new(),
                },
            ],
            break_policies: vec![BreakPolicy {
                id: id(21),
                name: "Lunch".to_owned(),
            }],
        }
    }

    #[test]
    fn only_catalogued_targets_are_accepted() {
        let catalog = catalog();
        assert!(catalog.target(id(1), Some(id(11))).is_ok());
        assert!(catalog.target(id(2), None).is_ok());
        assert_eq!(
            catalog.target(id(3), None),
            Err(CatalogError::UnknownProject)
        );
        // A task of another project, or an unknown one, is refused.
        assert_eq!(
            catalog.target(id(2), Some(id(11))),
            Err(CatalogError::UnknownTask)
        );
        assert_eq!(
            catalog.target(id(1), Some(id(12))),
            Err(CatalogError::UnknownTask)
        );
        assert_eq!(
            catalog.reassign_target(id(3)),
            Err(CatalogError::UnknownProject)
        );
        assert_eq!(
            catalog
                .break_policy(id(21))
                .map(|policy| policy.name.as_str()),
            Ok("Lunch")
        );
        assert_eq!(
            catalog.break_policy(id(1)).err(),
            Some(CatalogError::UnknownBreakPolicy)
        );
    }

    #[test]
    fn an_empty_catalog_accepts_nothing() {
        let catalog = Catalog::default();
        assert_eq!(
            catalog.target(id(1), None),
            Err(CatalogError::UnknownProject)
        );
        assert!(catalog.break_policy(id(21)).is_err());
    }
}
