#include "storage_bridge.hpp"

#include <dlfcn.h>
#include <openssl/evp.h>

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <memory>
#include <stdexcept>
#include <thread>
#include <vector>

namespace bambu_host {
namespace {

using json = nlohmann::json;
using Tunnel = void*;

struct Sample {
  int itrack;
  int size;
  int flags;
  const unsigned char* buffer;
  unsigned long long decode_time;
};

template <typename T>
T symbol(void* handle, const char* name) {
  void* raw = dlsym(handle, name);
  if (!raw) throw std::runtime_error(std::string("BambuSource missing ") + name);
  T result{};
  static_assert(sizeof(result) == sizeof(raw));
  std::memcpy(&result, &raw, sizeof(result));
  return result;
}

struct SourceApi {
  using Create = int(*)(Tunnel*, const char*);
  using Open = int(*)(Tunnel);
  using Start = int(*)(Tunnel, int);
  using Send = int(*)(Tunnel, int, const char*, int);
  using Read = int(*)(Tunnel, Sample*);
  using Close = void(*)(Tunnel);
  using Destroy = void(*)(Tunnel);

  void* handle;
  Create create;
  Open open;
  Start start;
  Send send;
  Read read;
  Close close;
  Destroy destroy;

  SourceApi() {
    const char* path = std::getenv("PJARCZAK_BAMBU_SOURCE_SO");
    if (!path || !*path) throw std::runtime_error("PJARCZAK_BAMBU_SOURCE_SO is not set");
    handle = dlopen(path, RTLD_LAZY | RTLD_GLOBAL);
    if (!handle) throw std::runtime_error(std::string("BambuSource dlopen failed: ") + dlerror());
    create = symbol<Create>(handle, "Bambu_Create");
    open = symbol<Open>(handle, "Bambu_Open");
    start = symbol<Start>(handle, "Bambu_StartStreamEx");
    send = symbol<Send>(handle, "Bambu_SendMessage");
    read = symbol<Read>(handle, "Bambu_ReadSample");
    close = symbol<Close>(handle, "Bambu_Close");
    destroy = symbol<Destroy>(handle, "Bambu_Destroy");
  }
};

SourceApi& api() { static SourceApi value; return value; }

class Session {
 public:
  explicit Session(const std::string& url) : api_(api()) {
    int rc = api_.create(&tunnel_, url.c_str());
    if (rc == 0) rc = api_.open(tunnel_);
    if (rc == 0) {
      auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
      do {
        rc = api_.start(tunnel_, 0x3001);
        if (rc == 2) std::this_thread::sleep_for(std::chrono::milliseconds(100));
      } while (rc == 2 && std::chrono::steady_clock::now() < deadline);
    }
    if (rc != 0) {
      cleanup();
      throw std::runtime_error("BambuSource connection failed rc=" + std::to_string(rc));
    }
  }
  ~Session() { cleanup(); }
  Session(const Session&) = delete;
  Session& operator=(const Session&) = delete;

  void request(int command, const json& request) {
    std::string body = json{{"cmdtype", command}, {"sequence", 1}, {"req", request}}.dump();
    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(5);
    int rc;
    do {
      rc = api_.send(tunnel_, 0x3001, body.data(), static_cast<int>(body.size()));
      if (rc == 2) std::this_thread::sleep_for(std::chrono::milliseconds(20));
    } while (rc == 2 && std::chrono::steady_clock::now() < deadline);
    if (rc != 0) throw std::runtime_error("BambuSource send failed rc=" + std::to_string(rc));
  }

  std::pair<json, std::vector<unsigned char>> response(
      std::chrono::steady_clock::time_point deadline) {
    while (std::chrono::steady_clock::now() < deadline) {
      Sample sample{};
      int rc = api_.read(tunnel_, &sample);
      if (rc == 2) {
        std::this_thread::sleep_for(std::chrono::milliseconds(20));
        continue;
      }
      if (rc != 0) throw std::runtime_error("BambuSource read failed rc=" + std::to_string(rc));
      if (!sample.buffer || sample.size <= 0) continue;
      const unsigned char separator[] = {'\n', '\n'};
      const unsigned char* begin = sample.buffer;
      const unsigned char* end = begin + sample.size;
      const unsigned char* split = std::search(begin, end, std::begin(separator), std::end(separator));
      const unsigned char* data = split == end ? end : split + 2;
      json root = json::parse(begin, split);
      return {std::move(root), std::vector<unsigned char>(data, end)};
    }
    throw std::runtime_error("BambuSource response timed out");
  }

 private:
  void cleanup() {
    if (tunnel_) {
      api_.close(tunnel_);
      api_.destroy(tunnel_);
      tunnel_ = nullptr;
    }
  }
  SourceApi& api_;
  Tunnel tunnel_ = nullptr;
};

const json& reply_of(const json& root) {
  static const json empty = json::object();
  if (root.contains("reply") && root["reply"].is_object()) return root["reply"];
  return empty;
}

void validate_remote(const std::string& value) {
  if (value.find('\0') != std::string::npos)
    throw std::runtime_error("invalid remote path");
  for (const auto& part : std::filesystem::path(value)) {
    if (part == "..") throw std::runtime_error("invalid remote path");
  }
}

std::string md5_file(const std::filesystem::path& path) {
  std::ifstream input(path, std::ios::binary);
  if (!input) throw std::runtime_error("cannot reopen downloaded file");
  std::unique_ptr<EVP_MD_CTX, decltype(&EVP_MD_CTX_free)> context(
      EVP_MD_CTX_new(), EVP_MD_CTX_free);
  if (!context || EVP_DigestInit_ex(context.get(), EVP_md5(), nullptr) != 1)
    throw std::runtime_error("cannot initialize MD5 validation");
  std::vector<char> buffer(64 * 1024);
  while (input) {
    input.read(buffer.data(), buffer.size());
    if (input.gcount() > 0 &&
        EVP_DigestUpdate(context.get(), buffer.data(), input.gcount()) != 1)
      throw std::runtime_error("cannot validate downloaded file");
  }
  unsigned char digest[EVP_MAX_MD_SIZE];
  unsigned int length = 0;
  if (EVP_DigestFinal_ex(context.get(), digest, &length) != 1)
    throw std::runtime_error("cannot finalize MD5 validation");
  static const char hex[] = "0123456789abcdef";
  std::string result;
  result.reserve(length * 2);
  for (unsigned int i = 0; i < length; ++i) {
    result.push_back(hex[digest[i] >> 4]);
    result.push_back(hex[digest[i] & 0x0f]);
  }
  return result;
}

}  // namespace

json StorageBridge::list(PluginLoader& loader, const std::string& dev_id,
                         const std::string& type, const std::string& storage) {
  std::lock_guard<std::mutex> guard(mutex_);
  if (type != "timelapse" && type != "video" && type != "model")
    throw std::runtime_error("invalid storage type");
  if (!storage.empty() && storage != "internal")
    throw std::runtime_error("invalid storage selector");
  Session session(loader.get_camera_url(dev_id));
  json request{{"type", type}, {"api_version", 2}, {"notify", "DETAIL"}};
  if (!storage.empty()) request["storage"] = storage;
  session.request(1, request);
  json files = json::array();
  auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(45);
  for (;;) {
    auto frame = session.response(deadline);
    const json& root = frame.first;
    int result = root.value("result", -1);
    const json& reply = reply_of(root);
    if (reply.contains("file_lists") && reply["file_lists"].is_array()) {
      for (const auto& file : reply["file_lists"]) files.push_back(file);
    }
    if (result == 0) break;
    if (result != 1) throw std::runtime_error("printer list failed result=" + std::to_string(result));
  }
  return {{"files", files}, {"type", type}, {"storage", storage}};
}

json StorageBridge::download(PluginLoader& loader, const std::string& dev_id,
                             const std::string& remote_path,
                             const std::string& remote_name,
                             const std::string& target_path,
                             std::uint64_t max_bytes) {
  std::lock_guard<std::mutex> guard(mutex_);
  validate_remote(remote_path);
  validate_remote(remote_name);
  const char* root_env = std::getenv("CLOUD_LAN_PROXY_REMOTE_CACHE_DIR");
  std::filesystem::path root(root_env ? root_env : "");
  if (root.empty()) throw std::runtime_error("remote cache directory is not configured");
  std::filesystem::path target = std::filesystem::weakly_canonical(target_path);
  std::filesystem::path canonical_root = std::filesystem::weakly_canonical(root);
  auto root_text = canonical_root.string() + "/";
  if (target.string().rfind(root_text, 0) != 0) throw std::runtime_error("target escapes remote cache");
  std::filesystem::create_directories(target.parent_path());
  std::filesystem::path partial = target;
  partial += ".part";
  std::ofstream output(partial, std::ios::binary | std::ios::trunc);
  if (!output) throw std::runtime_error("cannot create download target");
  try {
    Session session(loader.get_camera_url(dev_id));
    json request;
    if (!remote_path.empty()) request["path"] = remote_path;
    else request["file"] = remote_name;
    session.request(4, request);
    std::uint64_t written = 0;
    std::uint64_t total = 0;
    std::string md5;
    auto deadline = std::chrono::steady_clock::now() + std::chrono::minutes(20);
    for (;;) {
      auto frame = session.response(deadline);
      const json& reply = reply_of(frame.first);
      int result = frame.first.value("result", -1);
      std::uint64_t offset = reply.value("offset", written);
      total = reply.value("total", total);
      std::uint64_t size = reply.value("size", frame.second.size());
      if (offset != written || size != frame.second.size()) throw std::runtime_error("download frame mismatch");
      if (written + size > max_bytes) throw std::runtime_error("remote file exceeds download limit");
      output.write(reinterpret_cast<const char*>(frame.second.data()), frame.second.size());
      if (!output) throw std::runtime_error("download write failed");
      written += size;
      if (reply.contains("file_md5")) md5 = reply["file_md5"].get<std::string>();
      if (result == 0) break;
      if (result != 1) throw std::runtime_error("printer download failed result=" + std::to_string(result));
    }
    if (total != 0 && written != total) throw std::runtime_error("download size mismatch");
    output.close();
    if (!md5.empty()) {
      std::transform(md5.begin(), md5.end(), md5.begin(),
                     [](unsigned char c) { return std::tolower(c); });
      if (md5_file(partial) != md5) throw std::runtime_error("download checksum mismatch");
    }
    std::filesystem::rename(partial, target);
    std::filesystem::permissions(
        target,
        std::filesystem::perms::owner_read | std::filesystem::perms::owner_write |
            std::filesystem::perms::group_read | std::filesystem::perms::others_read,
        std::filesystem::perm_options::replace);
    return {{"path", target.string()}, {"size", written}, {"md5", md5}};
  } catch (...) {
    output.close();
    std::error_code ignored;
    std::filesystem::remove(partial, ignored);
    throw;
  }
}

}  // namespace bambu_host
