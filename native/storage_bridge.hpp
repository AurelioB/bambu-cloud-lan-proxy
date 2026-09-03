// Project-owned storage operations exposed through the private native RPC.
#pragma once

#include <cstdint>
#include <mutex>
#include <string>
#include <vector>

#include "plugin_loader.hpp"
#include <nlohmann/json.hpp>

namespace bambu_host {

class StorageBridge {
 public:
  nlohmann::json list(PluginLoader& loader, const std::string& dev_id,
                      const std::string& type, const std::string& storage);
  nlohmann::json download(PluginLoader& loader, const std::string& dev_id,
                          const std::string& remote_path,
                          const std::string& remote_name,
                          const std::string& target_path,
                          std::uint64_t max_bytes);
  nlohmann::json ability(PluginLoader& loader, const std::string& dev_id,
                         int api_version);
  nlohmann::json subfile(PluginLoader& loader, const std::string& dev_id,
                         const nlohmann::json& request,
                         const std::string& target_path,
                         std::uint64_t max_bytes);
  nlohmann::json remove(PluginLoader& loader, const std::string& dev_id,
                        const std::vector<std::string>& paths);
  nlohmann::json upload(PluginLoader& loader, const std::string& dev_id,
                        const std::string& storage,
                        const std::string& remote_path,
                        const std::string& source_path,
                        std::uint64_t max_bytes);

 private:
  std::mutex mutex_;
};

}  // namespace bambu_host
