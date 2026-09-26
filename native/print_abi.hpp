// Protocol layout, NOT a serializable object. Must match the pinned Bambu .so.
// Field ordering verified against Gateway's 02.05.02.58 ABI research and
// BambuStudio/OrcaSlicer declarations. See docs/gateway-independence.md.
#pragma once
#include <string>
#include <nlohmann/json.hpp>

struct PrintArguments {
  std::string dev_id, task_name, project_name, preset_name;
  std::string filename, config_filename;
  int plate_index = 0;
  std::string ftp_folder, ftp_file, ftp_file_md5;
  std::string nozzle_mapping, ams_mapping, ams_mapping2, ams_mapping_info, nozzles_info;
  std::string connection_type, comments;
  int origin_profile_id = 0, stl_design_id = 0;
  std::string origin_model_id, print_type, dst_file;
  std::string dev_name, dev_ip;
  bool use_ssl_for_ftp = false, use_ssl_for_mqtt = false;
  std::string username, password;
  bool task_bed_leveling = false, task_flow_cali = false, task_vibration_cali = false;
  bool task_layer_inspect = false, task_record_timelapse = false, task_use_ams = false;
  std::string task_bed_type, extra_options;
  int auto_bed_leveling = 0, auto_flow_cali = 0, auto_offset_cali = 0;
  int extruder_cali_manual_mode = -1;
  bool task_ext_change_assist = false, try_emmc_print = false;

  explicit PrintArguments(const nlohmann::json& p) {
#define FIELD(name) if (p.contains(#name)) p.at(#name).get_to(name)
    FIELD(dev_id); FIELD(task_name); FIELD(project_name); FIELD(preset_name);
    FIELD(filename); FIELD(config_filename); FIELD(plate_index);
    FIELD(ftp_folder); FIELD(ftp_file); FIELD(ftp_file_md5);
    FIELD(nozzle_mapping); FIELD(ams_mapping); FIELD(ams_mapping2);
    FIELD(ams_mapping_info); FIELD(nozzles_info); FIELD(connection_type);
    FIELD(comments); FIELD(origin_profile_id); FIELD(stl_design_id);
    FIELD(origin_model_id); FIELD(print_type); FIELD(dst_file);
    FIELD(dev_name); FIELD(dev_ip); FIELD(use_ssl_for_ftp); FIELD(use_ssl_for_mqtt);
    FIELD(username); FIELD(password); FIELD(task_bed_leveling); FIELD(task_flow_cali);
    FIELD(task_vibration_cali); FIELD(task_layer_inspect); FIELD(task_record_timelapse);
    FIELD(task_use_ams); FIELD(task_bed_type); FIELD(extra_options);
    FIELD(auto_bed_leveling); FIELD(auto_flow_cali); FIELD(auto_offset_cali);
    FIELD(extruder_cali_manual_mode); FIELD(task_ext_change_assist); FIELD(try_emmc_print);
#undef FIELD
  }
};
