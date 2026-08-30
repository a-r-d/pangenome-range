cmake_minimum_required(VERSION 3.20)

project(path_membership_oracle LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

set(GBWT_ROOT "" CACHE PATH "Prefix containing include/gbwt and lib/libgbwt.a")
set(SDSL_ROOT "" CACHE PATH "Prefix containing include/sdsl and lib/libsdsl.a")

find_path(GBWT_INCLUDE_DIR gbwt/gbwt.h HINTS "${GBWT_ROOT}" PATH_SUFFIXES include)
find_library(GBWT_LIBRARY NAMES gbwt HINTS "${GBWT_ROOT}" PATH_SUFFIXES lib)
find_path(SDSL_INCLUDE_DIR sdsl/simple_sds.hpp HINTS "${SDSL_ROOT}" PATH_SUFFIXES include)
find_library(SDSL_LIBRARY NAMES sdsl HINTS "${SDSL_ROOT}" PATH_SUFFIXES lib)
find_package(OpenMP REQUIRED)
find_package(PkgConfig REQUIRED)
pkg_check_modules(ZSTD REQUIRED IMPORTED_TARGET libzstd)

if(NOT GBWT_INCLUDE_DIR OR NOT GBWT_LIBRARY)
  message(FATAL_ERROR "GBWT was not found; set GBWT_ROOT")
endif()
if(NOT SDSL_INCLUDE_DIR OR NOT SDSL_LIBRARY)
  message(FATAL_ERROR "SDSL was not found; set SDSL_ROOT")
endif()

add_executable(path-membership-oracle src/main.cpp)
target_include_directories(path-membership-oracle PRIVATE "${GBWT_INCLUDE_DIR}" "${SDSL_INCLUDE_DIR}")
target_link_libraries(path-membership-oracle PRIVATE
  "${GBWT_LIBRARY}"
  "${SDSL_LIBRARY}"
  OpenMP::OpenMP_CXX
  PkgConfig::ZSTD
)
target_compile_options(path-membership-oracle PRIVATE -Wall -Wextra -Wpedantic -Werror)

include(CTest)
if(BUILD_TESTING)
  add_test(NAME oracle_help COMMAND path-membership-oracle --help)
endif()
