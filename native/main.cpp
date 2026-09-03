// Isolated adapter for the proprietary Bambu plugin. Application policy,
// credentials UI, printer state, retries, and routing belong to the daemon.
#include "plugin_loader.hpp"
#include "print_abi.hpp"
#include "storage_bridge.hpp"
#include <atomic>
#include <cstdlib>
#include <filesystem>
#include <future>
#include <iostream>
#include <mutex>
#include <thread>
#include <unistd.h>

using json = nlohmann::json;
using bambu_host::PluginLoader;
static std::mutex output_mutex;
static int output_fd = -1;
static std::atomic<bool> printing{false}, storage_busy{false};

static void output(const json& message) {
  std::lock_guard<std::mutex> guard(output_mutex);
  const auto line = message.dump() + "\n";
  std::size_t sent = 0;
  while (sent < line.size()) {
    auto n = ::write(output_fd, line.data() + sent, line.size() - sent);
    if (n <= 0) std::_Exit(1);
    sent += static_cast<std::size_t>(n);
  }
}
static void event(json value) { output({{"event", std::move(value)}}); }
static std::string environment(const char* name) {
  const char* value = std::getenv(name);
  if (!value || !*value) throw std::runtime_error("missing environment setting");
  return value;
}
static void check(int rc) {
  if (rc != 0) throw std::runtime_error("plugin returned nonzero status");
}

void PluginLoader::initialize() {
  if (library) throw std::runtime_error("already initialized");
  library = dlopen(environment("BAMBU_NETWORK_SO").c_str(), RTLD_LAZY | RTLD_GLOBAL);
  if (!library) throw std::runtime_error("plugin load failed");
  auto state = environment("BAMBU_PLUGIN_STATE");
  std::filesystem::create_directories(state);
  agent = symbol<void*(*)(std::string)>("create_agent")(state);
  if (!agent) throw std::runtime_error("agent creation failed");
  check(invoke<int>("set_config_dir", state));
  check(invoke<int>("init_log"));
  check(invoke<int>("set_cert_file", std::string("/etc/ssl/certs"),
                    std::string("ca-certificates.crt")));
  check(invoke<int>("set_country_code", environment("BAMBU_CLOUD_REGION")));
  check(invoke<int>("start"));
  std::function<void(std::string, std::string)> message = [](auto serial, auto payload) {
    event({{"kind", "OnMessage"}, {"dev_id", serial}, {"payload", payload}});
  };
  std::function<void(std::string)> connected = [](auto serial) {
    event({{"kind", "OnPrinterConnected"}, {"dev_id", serial}});
  };
  check(invoke<int>("set_on_message_fn", message));
  check(invoke<int>("set_on_printer_connected_fn", connected));
}

std::string PluginLoader::get_camera_url(const std::string& serial) {
  struct Reply { std::promise<std::string> value; std::atomic<bool> completed{false}; };
  auto reply = std::make_shared<Reply>();
  auto future = reply->value.get_future();
  std::function<void(std::string)> callback = [reply](auto value) {
    if (!reply->completed.exchange(true)) reply->value.set_value(std::move(value));
  };
  check(invoke<int>("get_camera_url", serial, callback));
  if (future.wait_for(std::chrono::seconds(30)) != std::future_status::ready)
    throw std::runtime_error("media authorization timed out");
  auto value = future.get();
  if (value.rfind("bambu:///", 0) != 0) throw std::runtime_error("invalid media URL");
  return value;
}

static json dispatch(PluginLoader& plugin, const std::string& method, const json& p) {
  if (method == "echo") return p;
  if (method == "init_plugin") { plugin.initialize(); return {{"bootstrap_rc", 0}}; }
  if (method == "is_user_login") return {{"is_login", plugin.invoke<bool>(method)}};
  if (method == "is_server_connected")
    return {{"connected", plugin.invoke<bool>(method)}, {"available", true}};
  if (method == "connect_server") return {{"rc", plugin.invoke<int>(method)}};
  if (method == "change_user")
    return {{"rc", plugin.invoke<int>(method, p.at("canonical_login").get<std::string>())}};
  if (method == "user_logout")
    return {{"rc", plugin.invoke<int>(method, p.value("with_backend_notify", true))}};
  if (method == "start_subscribe")
    return {{"rc", plugin.invoke<int>(method, p.at("module").get<std::string>())}};
  if (method == "add_subscribe")
    return {{"rc", plugin.invoke<int>(method, p.at("dev_ids").get<std::vector<std::string>>())}};
  if (method == "set_user_selected_machine")
    return {{"rc", plugin.invoke<int>(method, p.at("dev_id").get<std::string>())}};
  if (method == "set_extra_http_header")
    return {{"rc", plugin.invoke<int>(method,
      p.at("headers").get<std::map<std::string, std::string>>())}};
  if (method == "send_message")
    return {{"rc", plugin.invoke<int>(method, p.at("dev_id").get<std::string>(),
      p.at("payload").get<std::string>(), p.value("qos", 0), 0)}};
  if (method == "install_device_cert") {
    plugin.invoke<void>(method, p.at("dev_id").get<std::string>(), false);
    return {{"dispatched", true}};
  }
  if (method == "get_my_token" || method == "get_user_print_info") {
    unsigned int status = 0;
    std::string body;
    if (method == "get_my_token")
      check(plugin.invoke<int>(method, p.at("ticket").get<std::string>(), &status, &body));
    else check(plugin.invoke<int>(method, &status, &body));
    if (status != 200) throw std::runtime_error("Bambu HTTP request failed");
    return json::parse(body);
  }
  if (method == "start_print") {
    PrintArguments args(p);
    if (args.dev_id.empty() || !std::filesystem::is_regular_file(args.filename))
      throw std::runtime_error("invalid print arguments");
    using Progress = std::function<void(int, int, std::string)>;
    using Cancel = std::function<bool()>;
    using Wait = std::function<bool(int, std::string)>;
    using Start = int(*)(void*, PrintArguments, Progress, Cancel, Wait);
    auto start = plugin.symbol<Start>("start_print");
    if (printing.exchange(true)) return {{"rc", -98}};
    std::thread([start, agent = plugin.agent, args]() {
      int result = -99;
      try {
        Progress progress = [serial = args.dev_id](int stage, int code, std::string) {
          event({{"kind", "OnUpdateStatus"}, {"dev_id", serial},
                 {"stage", stage}, {"code", code}});
        };
        result = start(agent, args, progress, [] { return false; },
                       [](int, std::string) { return true; });
      } catch (...) { }
      // Completion is sent AFTER the blocking plugin call returns, so staging
      // data cannot be removed while a native worker still consumes it.
      printing.store(false);
      event({{"kind", "PrintComplete"}, {"dev_id", args.dev_id}, {"rc", result}});
    }).detach();
    return {{"rc", 0}};
  }
  static bambu_host::StorageBridge storage;
  auto serial = p.value("dev_id", std::string());
  if (method == "storage.list")
    return storage.list(plugin, serial, p.at("type"), p.value("storage", ""));
  if (method == "storage.ability")
    return storage.ability(plugin, serial, p.value("api_version", 2));
  if (method == "storage.download")
    return storage.download(plugin, serial, p.value("path", ""), p.value("name", ""),
                            p.at("target_path"), p.at("max_bytes"));
  if (method == "storage.subfile")
    return storage.subfile(plugin, serial, p.at("request"),
                           p.at("target_path"), p.at("max_bytes"));
  if (method == "storage.delete")
    return storage.remove(plugin, serial, p.at("paths").get<std::vector<std::string>>());
  if (method == "storage.upload")
    return storage.upload(plugin, serial, p.value("storage", ""), p.at("path"),
                          p.at("source_path"), p.at("max_bytes"));
  throw std::runtime_error("unsupported operation");
}

int main() {
  // Keep proprietary printf output out of the protocol and out of caller logs.
  output_fd = dup(STDOUT_FILENO);
  if (output_fd < 0 || dup2(STDERR_FILENO, STDOUT_FILENO) < 0) return 1;
  auto* plugin = new PluginLoader;  // lives until process exit, including workers
  std::string line;
  while (std::getline(std::cin, line)) {
    if (line.size() > 4 * 1024 * 1024) return 1;
    json request;
    try {
      request = json::parse(line);
      request.at("id").get<int>();
      request.at("method").get<std::string>();
    } catch (...) { return 1; }
    const auto method = request.at("method").get<std::string>();
    auto handle = [plugin, request, method] {
      try { output({{"id", request["id"]},
                    {"result", dispatch(*plugin, method, request.at("params"))}}); }
      catch (...) { output({{"id", request["id"]}, {"error", "native operation failed"}}); }
    };
    if (method.rfind("storage.", 0) == 0) {
      if (storage_busy.exchange(true)) {
        output({{"id", request["id"]}, {"error", "storage busy"}});
      } else {
        std::thread([handle] { handle(); storage_busy.store(false); }).detach();
      }
    } else handle();
  }
  // No dlclose/destruction while a proprietary callback/worker may still exist.
  std::_Exit(0);
}
