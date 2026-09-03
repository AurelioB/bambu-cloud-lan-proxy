// Bambu C++ ABI boundary. This is not a general printer-management library.
// The legacy class name keeps our pre-existing storage bridge source compatible.
#pragma once
#include <cstring>
#include <dlfcn.h>
#include <functional>
#include <map>
#include <stdexcept>
#include <string>
#include <nlohmann/json.hpp>

namespace bambu_host {
class PluginLoader {
 public:
  void* library = nullptr;
  void* agent = nullptr;

  template<class F> F symbol(const std::string& suffix) {
    void* address = dlsym(library, ("bambu_network_" + suffix).c_str());
    if (!address) throw std::runtime_error("missing plugin symbol: " + suffix);
    F result;
    static_assert(sizeof result == sizeof address);
    std::memcpy(&result, &address, sizeof result);
    return result;
  }

  template<class R, class... A> R invoke(const std::string& method, A... args) {
    if (!agent) throw std::runtime_error("plugin is not initialized");
    return symbol<R(*)(void*, A...)>(method)(agent, args...);
  }

  void initialize();
  std::string get_camera_url(const std::string& serial);
};
}
