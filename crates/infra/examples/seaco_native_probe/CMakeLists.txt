cmake_minimum_required(VERSION 3.16)
project(SaymoreSeacoNativeProbe LANGUAGES CXX)

if(NOT DEFINED FUNASR_ROOT OR NOT DEFINED FUNASR_BUILD OR NOT DEFINED ONNXRUNTIME_DIR)
    message(FATAL_ERROR "Set FUNASR_ROOT, FUNASR_BUILD, and ONNXRUNTIME_DIR to the isolated runtime paths")
endif()

find_library(FUNASR_LIBRARY NAMES funasr PATHS "${FUNASR_BUILD}/src" NO_DEFAULT_PATH REQUIRED)
add_executable(seaco_native_probe main.cpp)
target_compile_features(seaco_native_probe PRIVATE cxx_std_17)
target_include_directories(seaco_native_probe PRIVATE
    "${FUNASR_ROOT}/include"
    "${FUNASR_ROOT}/third_party/json/include")
target_link_libraries(seaco_native_probe PRIVATE "${FUNASR_LIBRARY}")
set_target_properties(seaco_native_probe PROPERTIES
    BUILD_RPATH "${FUNASR_BUILD}/src;${ONNXRUNTIME_DIR}/lib")
