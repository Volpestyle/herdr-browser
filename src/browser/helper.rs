//! Chromium's sandboxed renderer/GPU subprocess entry point (separate app bundle).
#[cfg(target_os = "macos")]
const WEBMCP_SHIM: &str = r#"(() => {
  const KEY = Symbol.for('herdr.webmcp');
  if (globalThis[KEY]) return;
  const tools = new Map();
  const registerTool = async (tool, opts) => {
    if (!tool || typeof tool.name !== 'string' || !tool.name || typeof tool.execute !== 'function') {
      throw new TypeError('WebMCP tool requires name and execute function');
    }
    const signal = opts?.signal;
    if (signal?.aborted) {
      const e = new Error('Aborted');
      e.name = 'AbortError';
      throw e;
    }
    const entry = {
      name: tool.name,
      description: typeof tool.description === 'string' ? tool.description : '',
      inputSchema: tool.inputSchema ?? { type: 'object', properties: {} },
      annotations: tool.annotations ?? {},
      execute: tool.execute,
    };
    tools.set(tool.name, entry);
    if (signal) {
      signal.addEventListener('abort', () => {
        if (tools.get(tool.name) === entry) tools.delete(tool.name);
      }, { once: true });
    }
  };
  const unregisterTool = async (name) => { tools.delete(name); };
  const provideContext = async (ctx) => {
    tools.clear();
    if (Array.isArray(ctx?.tools)) {
      for (const t of ctx.tools) await registerTool(t);
    }
  };
  const clearContext = async () => { tools.clear(); };
  const modelContext = Object.freeze({ registerTool, unregisterTool, provideContext, clearContext });
  const define = (target) => {
    if (!target) return;
    try {
      if (!('modelContext' in target) || target.modelContext == null) {
        Object.defineProperty(target, 'modelContext', { value: modelContext, configurable: true, enumerable: true });
      }
    } catch {}
  };
  define(globalThis.document);
  define(globalThis.navigator);
  if (typeof Document !== 'undefined' && !('modelContext' in Document.prototype)) {
    try { Object.defineProperty(Document.prototype, 'modelContext', { get: () => modelContext, configurable: true, enumerable: true }); } catch {}
  }
  Object.defineProperty(globalThis, KEY, {
    value: Object.freeze({
      list: () => [...tools.values()].map(({ name, description, inputSchema, annotations }) => ({ name, description, inputSchema, annotations })),
      call: async (name, input, timeoutMs) => {
        const entry = tools.get(name);
        if (!entry) throw new Error('Unknown WebMCP tool: ' + name);
        const ac = new AbortController();
        const timer = timeoutMs ? setTimeout(() => ac.abort(), timeoutMs) : null;
        try {
          return await entry.execute(input ?? {}, { signal: ac.signal });
        } finally {
          if (timer) clearTimeout(timer);
        }
      },
    }),
    configurable: false,
    enumerable: false,
  });
})();"#;

#[cfg(target_os = "macos")]
fn main() {
    use cef::{args::Args, *};

    wrap_render_process_handler! {
        struct WebMcpRender;
        impl RenderProcessHandler {
            fn on_context_created(
                &self,
                _browser: Option<&mut Browser>,
                frame: Option<&mut Frame>,
                context: Option<&mut V8Context>,
            ) {
                if !frame.is_some_and(|f| f.is_main() != 0) {
                    return;
                }
                if let Some(ctx) = context {
                    let code = CefString::from(WEBMCP_SHIM);
                    let url = CefString::from("herdr://webmcp.js");
                    let mut retval = None;
                    let mut exception = None;
                    let _ = ctx.eval(Some(&code), Some(&url), 1, Some(&mut retval), Some(&mut exception));
                }
            }
        }
    }

    wrap_app! {
        struct HelperApp;
        impl App {
            fn render_process_handler(&self) -> Option<RenderProcessHandler> {
                Some(WebMcpRender::new())
            }
        }
    }

    if std::env::args().any(|a| a == "--print-cef-path") {
        println!(
            "{}",
            cef::sys::get_cef_dir()
                .expect("Missing compiled CEF path")
                .display()
        );
        return;
    }
    let args = Args::new();
    let mut sandbox = cef::sandbox::Sandbox::new();
    sandbox.initialize(args.as_main_args());
    let loader = library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), true);
    assert!(loader.load(), "Cannot load the bundled Chromium framework");
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let mut app = HelperApp::new();
    std::process::exit(execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    ));
}

#[cfg(not(target_os = "macos"))]
fn main() {}
