#![recursion_limit = "256"]
#![allow(clippy::expect_fun_call)]
#![allow(clippy::let_and_return)]
#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]

//! delicate-scheduler.

#[macro_use]
extern crate diesel;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate diesel_migrations;

#[macro_use]
pub(crate) mod macros;
pub(crate) mod actions;
pub(crate) mod components;
pub(crate) mod db;
pub(crate) mod prelude;

pub(crate) use prelude::*;

#[actix_web::main]
async fn main() -> AnyResut<()> {
    // Loads environment variables.
    dotenv().ok();

    db::init();

    let scheduler_listening_address = env::var("SCHEDULER_LISTENING_ADDRESS")
        .expect("Without `SCHEDULER_LISTENING_ADDRESS` set in .env");

    let scheduler_front_end_domain: String = env::var("SCHEDULER_FRONT_END_DOMAIN")
        .expect("Without `SCHEDULER_FRONT_END_DOMAIN` set in .env");

    let log_level: Level =
        FromStr::from_str(&env::var("LOG_LEVEL").unwrap_or_else(|_| String::from("info")))
            .expect("Log level acquired fail.");

    // Prepare a `FileLogWriter` and a handle to it, and keep the handle alive
    // until the program ends (it will flush and shutdown the `FileLogWriter` when dropped).
    // For the `FileLogWriter`, use the settings that fit your needs
    let (file_writer, _fw_handle) = FileLogWriter::builder(FileSpec::default())
        .rotate(
            // If the program runs long enough,
            Criterion::Age(Age::Day),  // - create a new file every day
            Naming::Timestamps,        // - let the rotated files have a timestamp in their name
            Cleanup::KeepLogFiles(15), // - keep at most seven log files
        )
        .write_mode(WriteMode::Async)
        .try_build_with_handle()
        .expect("flexi_logger init failed");

    FmtSubscriber::builder()
        // will be written to file_writer.
        .with_max_level(log_level)
        .with_thread_names(true)
        .with_writer(move || file_writer.clone())
        // completes the builder.
        .init();

    let delay_timer = DelayTimerBuilder::default().enable_status_report().build();
    let shared_delay_timer = ShareData::new(delay_timer);

    let connection_pool = db::get_connection_pool();
    let shared_connection_pool = ShareData::new(connection_pool);
    let shared_scheduler_meta_info: SharedSchedulerMetaInfo =
        ShareData::new(SchedulerMetaInfo::default());

    #[cfg(AUTH_CASBIN)]
    let enforcer = get_casbin_enforcer(shared_connection_pool.clone()).await;
    #[cfg(AUTH_CASBIN)]
    let shared_enforcer = ShareData::new(RwLock::new(enforcer));

    // All ready work when the delicate-application starts.
    launch_ready_operation(
        shared_connection_pool.clone(),
        #[cfg(AUTH_CASBIN)]
        shared_enforcer.clone(),
    )
    .await;

    let result = HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin(&scheduler_front_end_domain)
            .allow_any_method()
            .allow_any_header()
            .supports_credentials()
            .max_age(3600);

        #[cfg(APP_DEBUG_MODE)]
        let cors = cors.allow_any_origin();

        let app = App::new()
            .configure(actions::task::config)
            .configure(actions::user::config)
            .configure(actions::task_log::config)
            .configure(actions::executor_group::config)
            .configure(actions::executor_processor::config)
            .configure(actions::executor_processor_bind::config)
            .configure(actions::data_reports::config)
            .configure(actions::components::config)
            .configure(actions::operation_log::config)
            .configure(actions::user_login_log::config)
            .app_data(shared_delay_timer.clone())
            .app_data(shared_connection_pool.clone())
            .app_data(shared_scheduler_meta_info.clone());

        #[cfg(AUTH_CASBIN)]
        let app = app
            .configure(actions::role::config)
            .wrap(CasbinService)
            .app_data(shared_enforcer.clone());

        app.wrap(components::session::auth_middleware())
            .wrap(components::session::session_middleware())
            .wrap(cors)
            .wrap(MiddlewareLogger::default())
            .wrap_fn(|req, srv| {
                let unique_id = get_unique_id_string();
                let unique_id_str = unique_id.deref();
                let fut = srv
                    .call(req)
                    .instrument(info_span!("log-id: ", unique_id_str));
                async {
                    let res = fut.await?;
                    Ok(res)
                }
            })
    })
    .bind(scheduler_listening_address)?
    .run()
    .await;

    Ok(result?)
}

// All ready work when the delicate-application starts.
async fn launch_ready_operation(
    pool: ShareData<db::ConnectionPool>,
    #[cfg(AUTH_CASBIN)] enforcer: ShareData<RwLock<Enforcer>>,
) {
    launch_health_check(pool.clone());
    launch_operation_log_consumer(pool);

    #[cfg(AUTH_CASBIN)]
    {
        // When the delicate starts, it checks if the resource acquisition is normal.
        let redis_url = env::var("REDIS_URL").expect("The redis url could not be acquired.");
        let redis_client = redis::Client::open(redis_url)
            .expect("The redis client resource could not be initialized.");
        launch_casbin_rule_events_consumer(redis_client, enforcer);
    }
}

// Heartbeat checker
// That constantly goes to detect whether the machine survives with the machine's indicators.
fn launch_health_check(pool: ShareData<db::ConnectionPool>) {
    rt_spawn(loop_health_check(pool));
}

// Operation log asynchronous consumer
//
// The user's operations in the system are logged to track,
// But in order not to affect the performance of the system,
// These logs go through the channel with the asynchronous state machine to consume.
fn launch_operation_log_consumer(pool: ShareData<db::ConnectionPool>) {
    rt_spawn(loop_operate_logs(pool));
}
