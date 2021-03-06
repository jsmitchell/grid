/*
 * Copyright 2019 Bitwise IO, Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 * -----------------------------------------------------------------------------
 */

#[macro_use]
extern crate clap;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate diesel_migrations;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_json;

mod config;
mod database;
mod error;
mod event;
mod rest_api;
#[cfg(feature = "sawtooth-support")]
mod sawtooth;
#[cfg(feature = "splinter-support")]
mod splinter;
mod submitter;

use std::process;
use std::sync::atomic::{AtomicBool, Ordering};

use log::Level;
use simple_logger;

use crate::config::GridConfigBuilder;
use crate::database::{error::DatabaseError, helpers as db};
use crate::error::DaemonError;
use crate::event::{db_handler::DatabaseEventHandler, EventProcessor};
use crate::sawtooth::{batch_submitter::SawtoothBatchSubmitter, connection::SawtoothConnection};

const APP_NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn run() -> Result<(), DaemonError> {
    let matches = clap_app!(myapp =>
        (name: APP_NAME)
        (version: VERSION)
        (author: "Contributors to Hyperledger Grid")
        (about: "Daemon Package for Hyperledger Grid")
        (@arg connect: -C --connect +takes_value "connection endpoint for sawtooth or splinter")
        (@arg verbose: -v +multiple "Log verbosely")
        (@arg database_url: --("database-url") +takes_value
         "specifies the database URL to connect to.")
        (@arg bind: -b --bind +takes_value "connection endpoint for rest API")
    )
    .get_matches();

    let log_level = match matches.occurrences_of("verbose") {
        0 => Level::Warn,
        1 => Level::Info,
        2 => Level::Debug,
        _ => Level::Trace,
    };
    simple_logger::init_with_level(log_level)?;

    let config = GridConfigBuilder::default()
        .with_cli_args(&matches)
        .build()?;

    let sawtooth_connection = if config.endpoint().is_sawtooth() {
        SawtoothConnection::new(&config.endpoint().url())
    } else {
        return Err(DaemonError::UnsupportedEndpoint(format!(
            "Unsupported endpoint type: {}",
            config.endpoint().url()
        )));
    };

    let connection_pool = database::create_connection_pool(config.database_url())?;

    let current_commit =
        db::get_current_commit_id(&*connection_pool.get()?).map_err(DatabaseError::from)?;

    let batch_submitter = Box::new(SawtoothBatchSubmitter::new(
        sawtooth_connection.get_sender(),
    ));

    let (rest_api_shutdown_handle, rest_api_join_handle) = rest_api::run(
        config.rest_api_endpoint(),
        connection_pool.clone(),
        batch_submitter,
        config.endpoint().clone(),
    )?;

    let evt_processor = EventProcessor::start(
        sawtooth_connection,
        &current_commit,
        event_handlers![DatabaseEventHandler::new(connection_pool)],
    )
    .map_err(|err| DaemonError::EventProcessorError(Box::new(err)))?;

    let (event_processor_shutdown_handle, event_processor_join_handle) =
        evt_processor.take_shutdown_controls();

    let ctrlc_triggered = AtomicBool::new(false);
    ctrlc::set_handler(move || {
        if ctrlc_triggered.load(Ordering::SeqCst) {
            eprintln!("Aborting due to multiple Ctrl-C events");
            process::exit(1);
        }

        ctrlc_triggered.store(true, Ordering::SeqCst);
        if let Err(err) = rest_api_shutdown_handle.shutdown() {
            error!("Unable to cleanly shutdown REST API server: {}", err);
        }
        if let Err(err) = event_processor_shutdown_handle.shutdown() {
            error!("Unable to gracefully shutdown Event Processor: {}", err);
        }
    })
    .map_err(|err| DaemonError::StartUpError(Box::new(err)))?;

    rest_api_join_handle
        .join()
        .map_err(|_| {
            DaemonError::ShutdownError("Unable to cleanly join the REST API thread".into())
        })
        .and_then(|res| res.map_err(DaemonError::from))?;

    event_processor_join_handle
        .join()
        .map_err(|_| {
            DaemonError::ShutdownError("Unable to cleanly join the event processor".into())
        })
        .and_then(|res| res.map_err(DaemonError::from))?;

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        error!("{}", e);
        std::process::exit(1);
    }
}
