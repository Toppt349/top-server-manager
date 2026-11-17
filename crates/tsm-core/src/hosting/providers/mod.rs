pub mod basic;
pub mod cloudflare;
pub mod frp;
pub mod ngrok;
pub mod playit;

pub use basic::{DirectIpProvider, LanProvider, LocalProvider, MockProvider, TailscaleProvider};
pub use cloudflare::CloudflareProvider;
pub use frp::FrpProvider;
pub use ngrok::NgrokProvider;
pub use playit::PlayitProvider;

use super::agent::AgentCtx;
use super::HostingProvider;
use std::sync::Arc;

pub fn all(ctx: AgentCtx) -> Vec<Arc<dyn HostingProvider>> {
    vec![
        Arc::new(LocalProvider::default()),
        Arc::new(LanProvider::default()),
        Arc::new(DirectIpProvider::default()),
        Arc::new(CloudflareProvider::new(ctx.clone())),
        Arc::new(NgrokProvider::new(ctx.clone())),
        Arc::new(PlayitProvider::new(ctx.clone())),
        Arc::new(FrpProvider::new(ctx.clone())),
        Arc::new(TailscaleProvider::new(ctx)),
    ]
}
