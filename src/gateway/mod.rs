mod client;
mod server;
mod solar;
mod store;
pub use solar::{SolarContext, SunPhase};
mod weather;

pub use client::{
    FeedOutcome, FeedRefusal, FeedRequestStatus, FeederSafety, GatewayClient, RefusalReason,
    WeatherSnapshot,
};
pub use server::{GatewayServerConfig, TrustedGateway};
pub use weather::format_weather_message;
pub(crate) use weather::{now_epoch, validate_observation_time};
