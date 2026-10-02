//! [`GitHubClient::stacks`]: the one Stacks API call rostrum makes.
//!
//! A child of `client` so it can use the request helpers without widening
//! their visibility; the decoding lives in [`crate::stacks`].

use reqwest::{Method, StatusCode};
use rostrum_core::RepoId;

use crate::{
    error::GitHubError,
    stacks::{RepoStacks, parse_stacks},
};

use super::{GitHubClient, MAX_PAGES, next_page_url};

impl GitHubClient {
    /// The repository's open stacks, following pagination to the end.
    ///
    /// A 404 is [`RepoStacks::Unavailable`] — stacked pull requests are not
    /// enabled for the repository — rather than an error, so the caller can
    /// stop asking instead of reporting a failure every poll.
    pub async fn stacks(&self, repo: &RepoId) -> Result<RepoStacks, GitHubError> {
        let resource = format!("stacks for {repo}");
        let mut url = format!(
            "{}/repos/{}/{}/stacks?per_page=100",
            self.rest_base,
            repo.owner(),
            repo.name()
        );

        let mut stacks = Vec::new();
        for _ in 0..MAX_PAGES {
            let response = self.execute(self.rest(Method::GET, &url)).await?;
            if response.status == StatusCode::NOT_FOUND {
                return Ok(RepoStacks::Unavailable);
            }
            response.check_status(&resource)?;
            stacks.extend(parse_stacks(repo, &response.body)?);

            match next_page_url(&response.headers) {
                Some(next) => url = next,
                None => break,
            }
        }
        Ok(RepoStacks::Available(stacks))
    }
}
