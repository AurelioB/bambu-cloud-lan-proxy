#pragma once

#include <cstdint>
#include <mutex>
#include <string>

#include "plugin_loader.hpp"
#include "third_party/nlohmann/json.hpp"

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

 private:
  std::mutex mutex_;
};

}  // namespace bambu_host
