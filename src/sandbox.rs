//! The limits every extension runs under.
//!
//! An extension gets no filesystem and no environment, a bounded amount of
//! linear memory, a fuel budget and a wall-clock deadline for each host
//! call, and outgoing HTTP only when its manifest declares the `network`
//! capability.
//!
//! Fuel is consumed as the guest executes and yields to the host at a fixed
//! interval, so a guest stuck in a loop still lets the per-call deadline
//! fire. A call that runs out of fuel traps and reports an error; a call
//! that outlives its deadline stops the worker, and later calls report
//! [`Error::ExtensionCrashed`](crate::Error::ExtensionCrashed).

use std::future::Future;
use std::time::Duration;

use wasmtime::{Config, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi_http::{RequestOptions, WasiBody, WasiHttpHooks};

use crate::Error;
use crate::manifest::Manifest;

/// The manifest capability that grants outgoing HTTP.
pub const NETWORK: &str = "network";

/// Resource limits for one extension instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    memory: usize,
    fuel: u64,
    deadline: Duration,
}

impl Default for Limits {
    /// 256 MiB of memory, about ten billion instructions and 60 seconds per
    /// call: far above what a connector spends waiting on its API, far below
    /// what lets a runaway guest hang the host.
    fn default() -> Self {
        Self {
            memory: 256 * 1024 * 1024,
            fuel: 10_000_000_000,
            deadline: Duration::from_secs(60),
        }
    }
}

impl Limits {
    /// The most linear memory, in bytes, the instance may grow to.
    pub fn memory(self, bytes: usize) -> Self {
        Self { memory: bytes, ..self }
    }

    /// The fuel each call starts with.
    pub fn fuel(self, fuel: u64) -> Self {
        Self { fuel, ..self }
    }

    /// The wall-clock time each call may take.
    pub fn deadline(self, deadline: Duration) -> Self {
        Self { deadline, ..self }
    }
}

/// How often, in fuel units, a running guest yields to the host executor.
const YIELD_INTERVAL: u64 = 1_000_000;

/// The limits and grants for one extension, derived from its manifest.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sandbox {
    limits: Limits,
    network: bool,
}

impl Sandbox {
    /// Grant what `manifest` declares, under `limits`.
    pub(crate) fn new(manifest: &Manifest, limits: Limits) -> Self {
        Self {
            limits,
            network: manifest.capabilities.get(NETWORK).copied().unwrap_or(false),
        }
    }

    /// Turn on the engine features the limits rely on.
    pub(crate) fn configure(&self, config: &mut Config) {
        config.consume_fuel(true);
    }

    /// The per-store guard: the memory limiter and the network grant.
    pub(crate) fn guard(&self) -> Guard {
        Guard {
            limits: StoreLimitsBuilder::new()
                .memory_size(self.limits.memory)
                .trap_on_grow_failure(true)
                .build(),
            network: Network { allowed: self.network },
        }
    }

    /// Install the guard's limiter on `store` and give it its first fuel.
    pub(crate) fn arm<T: AsMut<Guard>>(&self, store: &mut Store<T>) -> Result<(), Error> {
        store.limiter(|state| &mut state.as_mut().limits);
        store.fuel_async_yield_interval(Some(YIELD_INTERVAL))?;
        self.refuel(store)
    }

    /// Give `store` a full budget before the next call.
    pub(crate) fn refuel<T>(&self, store: &mut Store<T>) -> Result<(), Error> {
        store.set_fuel(self.limits.fuel)?;
        Ok(())
    }

    /// Run one call against its deadline. `Err` means the deadline passed
    /// and the instance must not be entered again.
    pub(crate) async fn call<F: Future<Output = ()>>(&self, call: F) -> Result<(), Elapsed> {
        tokio::time::timeout(self.limits.deadline, call)
            .await
            .map_err(|_| Elapsed(self.limits.deadline))
    }
}

/// A call outlived its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Elapsed(pub(crate) Duration);

/// Per-store state the sandbox owns.
pub(crate) struct Guard {
    limits: StoreLimits,
    network: Network,
}

impl Guard {
    /// The HTTP hooks that enforce the network grant.
    pub(crate) fn network(&mut self) -> &mut Network {
        &mut self.network
    }
}

/// WASI-HTTP hooks that send requests only for extensions granted network.
pub(crate) struct Network {
    allowed: bool,
}

type Sent = Box<
    dyn Future<
            Output = wasmtime_wasi_http::Result<(
                http::Response<WasiBody>,
                Box<dyn Future<Output = wasmtime_wasi_http::Result<()>> + Send>,
            )>,
        > + Send,
>;

impl WasiHttpHooks for Network {
    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        options: Option<RequestOptions>,
        fut: Box<dyn Future<Output = wasmtime_wasi_http::Result<()>> + Send>,
    ) -> Sent {
        if self.allowed {
            return wasmtime_wasi_http::default_hooks().send_request(request, options, fut);
        }
        tracing::warn!(
            host = request.uri().host().unwrap_or_default(),
            "extension without the network capability tried to send a request"
        );
        Box::new(async { Err(wasmtime_wasi_http::Error::HttpRequestDenied) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::{Engine, Instance, Module};

    struct Host(Guard);

    impl AsMut<Guard> for Host {
        fn as_mut(&mut self) -> &mut Guard {
            &mut self.0
        }
    }

    const GUEST: &str = r#"
        (module
          (memory 1)
          (func (export "spin") (loop br 0))
          (func (export "grow") (result i32) (memory.grow (i32.const 16384)))
          (func (export "small") (result i32) (memory.grow (i32.const 1))))
    "#;

    fn manifest(network: Option<bool>) -> Manifest {
        let capabilities = network
            .map(|n| format!("[capabilities]\nnetwork = {n}\n"))
            .unwrap_or_default();
        let toml = format!(
            "[extension]\nid = \"test\"\nname = \"Test\"\nversion = \"0.1.0\"\nauthor = \"Test\"\nlicense = \"MIT\"\n\n[component]\nentry = \"x.wasm\"\n\n[config]\nschema = '{{}}'\n\n{capabilities}"
        );
        toml::from_str::<crate::manifest::RawManifest>(&toml)
            .unwrap()
            .into_manifest()
            .unwrap()
    }

    async fn instance(sandbox: &Sandbox) -> (Store<Host>, Instance) {
        let mut config = Config::new();
        sandbox.configure(&mut config);
        let engine = Engine::new(&config).unwrap();
        let module = Module::new(&engine, GUEST).unwrap();
        let mut store = Store::new(&engine, Host(sandbox.guard()));
        sandbox.arm(&mut store).unwrap();
        let instance = Instance::new_async(&mut store, &module, &[]).await.unwrap();
        (store, instance)
    }

    #[tokio::test]
    async fn a_spinning_guest_runs_out_of_fuel() {
        let sandbox = Sandbox::new(&manifest(None), Limits::default().fuel(5_000_000));
        let (mut store, instance) = instance(&sandbox).await;
        let spin = instance.get_typed_func::<(), ()>(&mut store, "spin").unwrap();
        let error = spin.call_async(&mut store, ()).await.unwrap_err();
        assert_eq!(error.downcast_ref::<wasmtime::Trap>(), Some(&wasmtime::Trap::OutOfFuel));
    }

    #[tokio::test]
    async fn a_spinning_guest_meets_its_deadline() {
        let limits = Limits::default().fuel(u64::MAX).deadline(Duration::from_millis(50));
        let sandbox = Sandbox::new(&manifest(None), limits);
        let (mut store, instance) = instance(&sandbox).await;
        let spin = instance.get_typed_func::<(), ()>(&mut store, "spin").unwrap();
        let outcome = sandbox
            .call(async {
                let _ = spin.call_async(&mut store, ()).await;
            })
            .await;
        assert_eq!(outcome, Err(Elapsed(Duration::from_millis(50))));
    }

    #[tokio::test]
    async fn memory_cannot_grow_past_the_limit() {
        let sandbox = Sandbox::new(&manifest(None), Limits::default().memory(2 * 65536));
        let (mut store, instance) = instance(&sandbox).await;
        let small = instance.get_typed_func::<(), i32>(&mut store, "small").unwrap();
        assert_eq!(
            small.call_async(&mut store, ()).await.unwrap(),
            1,
            "growing within the limit works"
        );
        let grow = instance.get_typed_func::<(), i32>(&mut store, "grow").unwrap();
        assert!(
            grow.call_async(&mut store, ()).await.is_err(),
            "growing past the limit traps"
        );
    }

    #[tokio::test]
    async fn refuel_restores_the_budget() {
        let sandbox = Sandbox::new(&manifest(None), Limits::default().fuel(1_000));
        let (mut store, _instance) = instance(&sandbox).await;
        store.set_fuel(0).unwrap();
        sandbox.refuel(&mut store).unwrap();
        assert_eq!(store.get_fuel().unwrap(), 1_000);
    }

    #[test]
    fn network_follows_the_manifest() {
        assert!(Sandbox::new(&manifest(Some(true)), Limits::default()).network);
        assert!(!Sandbox::new(&manifest(Some(false)), Limits::default()).network);
        assert!(
            !Sandbox::new(&manifest(None), Limits::default()).network,
            "undeclared means denied"
        );
    }

    #[tokio::test]
    async fn requests_without_the_network_grant_are_denied() {
        let mut network = Network { allowed: false };
        let request = http::Request::builder()
            .uri("https://example.com/")
            .body(WasiBody::default())
            .unwrap();
        let sent = Box::into_pin(network.send_request(request, None, Box::new(async { Ok(()) })));
        assert!(matches!(sent.await, Err(wasmtime_wasi_http::Error::HttpRequestDenied)));
    }
}
