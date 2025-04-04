use std::env;

use anyhow::{Context, Result};
use dotenvy::dotenv;
use gradescope_api::client::{client, Client};
use gradescope_api::course::Course;
use gradescope_api::course_selector::CourseSelector;
use gradescope_api::creds::Creds;
use gradescope_api::services::gs_service::GsService;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, registry, EnvFilter};

pub async fn init_from_env() -> Result<InitFromEnv<impl GsService>> {
    dotenv().unwrap();

    let course_selector = course_selector_from_env();

    let creds = Creds::from_env()?;
    let cache_path = env::var("CACHE_PATH")?.into();

    let gradescope = client(creds, cache_path).await?;

    let courses = gradescope.get_courses().await?;
    let course = course_selector
        .select_from(&courses)
        .with_context(|| format!("could not find course with selector {course_selector:?}"))?
        .clone();

    Ok(InitFromEnv { course, gradescope })
}

pub struct InitFromEnv<Service> {
    pub course: Course,
    pub gradescope: Client<Service>,
}

pub fn db_url_from_env() -> String {
    env::var("DATABASE_URL").unwrap()
}

fn course_selector_from_env() -> CourseSelector {
    CourseSelector::new(env::var("COURSE").unwrap())
}

pub fn init_tracing() {
    registry()
        .with(fmt::layer().event_format(format().pretty()))
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env()
                .unwrap(),
        )
        .init();
}
