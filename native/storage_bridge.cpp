// Project-owned Studio media protocol adapter.
#include "storage_bridge.hpp"

#include <dlfcn.h>
#include <openssl/evp.h>

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <memory>
#include <sstream>
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
    const char* path = std::getenv("BAMBU_SOURCE_SO");
    if (!path || !*path) throw std::runtime_error("BAMBU_SOURCE_SO is not set");
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
    if (!connect(url))
      throw std::runtime_error("BambuSource connection failed rc=" +
                               std::to_string(last_error_));
  }
  Session(PluginLoader& loader, const std::string& dev_id) : api_(api()) {
    if (auto local = loader.get_local_media_url(dev_id); local && connect(*local))
      return;
    cleanup();
    if (!connect(loader.get_camera_url(dev_id)))
      throw std::runtime_error("BambuSource connection failed rc=" +
                               std::to_string(last_error_));
  }
  ~Session() { cleanup(); }
  Session(const Session&) = delete;
  Session& operator=(const Session&) = delete;

 private:
  bool connect(const std::string& url) {
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
      last_error_ = rc;
      cleanup();
      return false;
    }
    last_error_ = 0;
    return true;
  }

 public:

  void request(int command, const json& request,
               const unsigned char* data = nullptr, std::size_t data_size = 0) {
    std::string body = json{{"cmdtype", command}, {"sequence", 1}, {"req", request}}.dump();
    if (data_size) {
      body += "\n\n";
      body.append(reinterpret_cast<const char*>(data), data_size);
    }
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
  int last_error_ = 0;
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

void validate_storage(const std::string& value) {
  if (value.size() > 32 || !std::all_of(value.begin(), value.end(), [](unsigned char c) {
        return std::isalnum(c) || c == '_' || c == '-';
      }))
    throw std::runtime_error("invalid storage selector");
}

std::filesystem::path confined_path(const char* root_name,
                                    const std::string& value) {
  const char* root_env = std::getenv(root_name);
  std::filesystem::path root(root_env ? root_env : "");
  if (root.empty()) throw std::runtime_error(std::string(root_name) + " is not configured");
  std::filesystem::path target = std::filesystem::weakly_canonical(value);
  std::filesystem::path canonical_root = std::filesystem::weakly_canonical(root);
  auto root_text = canonical_root.string() + "/";
  if (target.string().rfind(root_text, 0) != 0)
    throw std::runtime_error("path escapes configured root");
  return target;
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
  validate_storage(storage);
  Session session(loader, dev_id);
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
  std::filesystem::path target = confined_path(
      "CLOUD_LAN_PROXY_REMOTE_CACHE_DIR", target_path);
  std::filesystem::create_directories(target.parent_path());
  std::filesystem::path partial = target;
  partial += ".part";
  std::ofstream output(partial, std::ios::binary | std::ios::trunc);
  if (!output) throw std::runtime_error("cannot create download target");
  try {
    Session session(loader, dev_id);
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

json StorageBridge::ability(PluginLoader& loader, const std::string& dev_id,
                            int api_version) {
  std::lock_guard<std::mutex> guard(mutex_);
  if (api_version < 1 || api_version > 10)
    throw std::runtime_error("invalid media API version");
  Session session(loader, dev_id);
  session.request(7, {{"peer", "studio"}, {"api_version", api_version}});
  auto frame = session.response(std::chrono::steady_clock::now() + std::chrono::seconds(30));
  int result = frame.first.value("result", -1);
  if (result != 0) throw std::runtime_error("printer ability failed result=" + std::to_string(result));
  return {{"storage", reply_of(frame.first).value("storage", json::array())}};
}

json StorageBridge::subfile(PluginLoader& loader, const std::string& dev_id,
                            const json& request, const std::string& target_path,
                            std::uint64_t max_bytes) {
  std::lock_guard<std::mutex> guard(mutex_);
  if (!request.is_object()) throw std::runtime_error("invalid subfile request");
  if (request.contains("paths")) {
    if (!request["paths"].is_array() || request["paths"].size() > 64)
      throw std::runtime_error("invalid subfile paths");
    for (const auto& path : request["paths"]) {
      if (!path.is_string()) throw std::runtime_error("invalid subfile path");
      validate_remote(path.get<std::string>());
    }
  } else {
    validate_remote(request.value("path", std::string()));
    validate_remote(request.value("name", std::string()));
  }
  std::filesystem::path target = confined_path(
      "CLOUD_LAN_PROXY_REMOTE_CACHE_DIR", target_path);
  std::filesystem::path partial = target;
  partial += ".part";
  std::ofstream output(partial, std::ios::binary | std::ios::trunc);
  if (!output) throw std::runtime_error("cannot create subfile target");
  try {
    Session session(loader, dev_id);
    session.request(2, request);
    std::uint64_t written = 0;
    auto deadline = std::chrono::steady_clock::now() + std::chrono::minutes(5);
    for (;;) {
      auto frame = session.response(deadline);
      int result = frame.first.value("result", -1);
      const json& reply = reply_of(frame.first);
      std::uint64_t size = reply.value("size", frame.second.size());
      if (size != frame.second.size() || written + size > max_bytes)
        throw std::runtime_error("invalid or oversized subfile frame");
      output.write(reinterpret_cast<const char*>(frame.second.data()), frame.second.size());
      if (!output) throw std::runtime_error("subfile write failed");
      written += size;
      if (result == 0) break;
      if (result != 1) throw std::runtime_error("printer subfile failed result=" + std::to_string(result));
    }
    output.close();
    std::filesystem::rename(partial, target);
    return {{"path", target.string()}, {"size", written}};
  } catch (...) {
    output.close();
    std::error_code ignored;
    std::filesystem::remove(partial, ignored);
    throw;
  }
}

json StorageBridge::remove(PluginLoader& loader, const std::string& dev_id,
                           const std::vector<std::string>& paths) {
  std::lock_guard<std::mutex> guard(mutex_);
  if (paths.empty() || paths.size() > 100)
    throw std::runtime_error("invalid delete request");
  for (const auto& path : paths) validate_remote(path);
  if (std::any_of(paths.begin(), paths.end(), [](const std::string& path) {
        auto lower = path;
        std::transform(lower.begin(), lower.end(), lower.begin(),
                       [](unsigned char value) { return std::tolower(value); });
        return lower.find("/userdata/model/history/") != std::string::npos;
      }))
    throw std::runtime_error(
        "internal model history deletion is unsupported; use the printer UI");
  Session session(loader, dev_id);
  session.request(3, {{"paths", paths}});
  auto frame = session.response(std::chrono::steady_clock::now() + std::chrono::seconds(60));
  int result = frame.first.value("result", -1);
  if (result == 2) {
    // Studio has used both the batched `paths` form and the legacy
    // `delete`-by-name form. Some remote tunnels advertise absolute paths
    // but only accept the latter after a list request established the
    // current storage view on that tunnel.
    for (const auto& path : paths) {
      std::filesystem::path parsed(path);
      auto name = parsed.filename().string();
      auto lower = path;
      std::transform(lower.begin(), lower.end(), lower.begin(),
                     [](unsigned char value) { return std::tolower(value); });
      std::string type = lower.find("timelapse") != std::string::npos
                             ? "timelapse"
                         : lower.find("video") != std::string::npos ? "video"
                                                                    : "model";
      std::string storage = lower.find("usb") != std::string::npos ? "udisk"
                                                                    : "emmc";
      if (name.empty())
        throw std::runtime_error("invalid delete request");
      session.request(1, {{"api_version", 2}, {"notify", "DETAIL"},
                          {"type", type}, {"storage", storage}});
      frame = session.response(std::chrono::steady_clock::now() +
                               std::chrono::seconds(60));
      result = frame.first.value("result", -1);
      if (result != 0) break;
      session.request(3, {{"delete", std::vector<std::string>{name}}});
      frame = session.response(std::chrono::steady_clock::now() +
                               std::chrono::seconds(60));
      result = frame.first.value("result", -1);
      if (result != 0) break;
      session.request(1, {{"api_version", 2}, {"notify", "DETAIL"},
                          {"type", type}, {"storage", storage}});
      frame = session.response(std::chrono::steady_clock::now() +
                               std::chrono::seconds(60));
      result = frame.first.value("result", -1);
      if (result != 0) break;
      const auto& files = reply_of(frame.first).value("file_lists", json::array());
      if (std::any_of(files.begin(), files.end(), [&](const json& file) {
            return file.value("path", "") == path ||
                   file.value("name", "") == name;
          }))
        throw std::runtime_error("printer acknowledged delete but file remains");
    }
  }
  if (result != 0) throw std::runtime_error("printer delete failed result=" + std::to_string(result));
  return {{"deleted", paths.size()}};
}

json StorageBridge::upload(PluginLoader& loader, const std::string& dev_id,
                           const std::string& storage,
                           const std::string& remote_path,
                           const std::string& source_path,
                           std::uint64_t max_bytes) {
  std::lock_guard<std::mutex> guard(mutex_);
  validate_storage(storage);
  validate_remote(remote_path);
  std::filesystem::path source = confined_path(
      "CLOUD_LAN_PROXY_STAGING_DIR", source_path);
  if (!std::filesystem::is_regular_file(source))
    throw std::runtime_error("upload source is unavailable");
  std::uint64_t total = std::filesystem::file_size(source);
  if (total == 0 || total > max_bytes)
    throw std::runtime_error("upload source is empty or exceeds limit");
  std::string md5 = md5_file(source);
  Session session(loader, dev_id);
  session.request(5, {{"type", "model"}, {"storage", storage},
                      {"path", remote_path}, {"total", total}});
  auto init = session.response(std::chrono::steady_clock::now() + std::chrono::seconds(60));
  int init_result = init.first.value("result", -1);
  if (init_result != 1 && init_result != 19)
    throw std::runtime_error("printer upload init failed result=" + std::to_string(init_result));
  const json& reply = reply_of(init.first);
  std::uint64_t chunk_size = reply.value("chunk_size", std::uint64_t(255)) * 1024;
  std::uint64_t offset = reply.value("offset", std::uint64_t(0));
  if (chunk_size == 0 || chunk_size > 1024 * 1024 || offset > total)
    throw std::runtime_error("printer returned invalid upload parameters");
  std::ifstream input(source, std::ios::binary);
  input.seekg(static_cast<std::streamoff>(offset));
  std::vector<unsigned char> buffer(chunk_size);
  std::uint64_t frag_id = offset / chunk_size;
  while (offset < total) {
    std::uint64_t wanted = std::min<std::uint64_t>(chunk_size, total - offset);
    input.read(reinterpret_cast<char*>(buffer.data()), static_cast<std::streamsize>(wanted));
    if (static_cast<std::uint64_t>(input.gcount()) != wanted)
      throw std::runtime_error("upload source read failed");
    json req{{"frag_id", frag_id++}, {"offset", offset}, {"size", wanted}};
    if (offset + wanted == total) req["file_md5"] = md5;
    session.request(5, req, buffer.data(), wanted);
    offset += wanted;
  }
  auto done = session.response(std::chrono::steady_clock::now() + std::chrono::minutes(5));
  int result = done.first.value("result", -1);
  if (result != 0) throw std::runtime_error("printer upload failed result=" + std::to_string(result));
  return {{"uploaded", remote_path}, {"size", total}, {"md5", md5}};
}

}  // namespace bambu_host
