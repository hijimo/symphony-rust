//! Platform access for the document flow. Indirection through a factory so
//! integration tests can replace the git platform with an in-memory fake.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::WebPlatformError;
use crate::handlers::issues::map_platform_error;
use crate::models::Project;
use crate::proxy::EffectiveProxyConfig;
use crate::services::git_platform::{create_platform_client_with_proxy, GitPlatformClient};

#[async_trait]
pub trait PlatformFactory: Send + Sync {
    /// Client for a project (proxy applied).
    async fn client(
        &self,
        project: &Project,
        proxy: &EffectiveProxyConfig,
    ) -> Result<Arc<dyn GitPlatformClient>, WebPlatformError>;

    /// HTTPS clone URL of `namespace/repo` on the project's platform.
    fn clone_url(&self, project: &Project, slug: &str) -> String;
}

pub struct DefaultPlatformFactory;

#[async_trait]
impl PlatformFactory for DefaultPlatformFactory {
    async fn client(
        &self,
        project: &Project,
        proxy: &EffectiveProxyConfig,
    ) -> Result<Arc<dyn GitPlatformClient>, WebPlatformError> {
        if project.platform == "gitea" {
            return Err(WebPlatformError::BadRequest(
                "Gitea 项目的文档流程暂未接入平台 API".to_string(),
            ));
        }
        let c = create_platform_client_with_proxy(
            &project.platform,
            project.platform_host.as_deref(),
            Some(proxy),
        )
        .map_err(map_platform_error)?;
        Ok(Arc::from(c))
    }

    fn clone_url(&self, project: &Project, slug: &str) -> String {
        let host = crate::services::project_repos::effective_platform_host(project);
        format!("{}/{}.git", host.trim_end_matches('/'), slug)
    }
}
