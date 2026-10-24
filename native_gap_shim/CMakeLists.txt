# The mlx-c-rust gap shim: compiled by the native build engine as the
# third component after MLX and MLX-C, into the same machine-shared
# prefix. Static like the pair (consumer binaries embed it), C++20 like
# both upstreams, and found through the prefix's installed MLX package
# config plus a plain find_library for mlxc — the same resolution the
# MLX-C build itself uses for MLX.

cmake_minimum_required(VERSION 3.16)

project(mlxrs_gap_shim LANGUAGES CXX C)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(jaccl CONFIG REQUIRED)
# MLX's exported target links jaccl::jaccl, but MLX's config does not find
# jaccl itself — the same packaging gap the native build engine patches by
# writing jacclConfig.cmake into the prefix. Finding jaccl FIRST defines
# the target find_package(MLX) then needs.
find_package(MLX REQUIRED)

find_library(
  MLXC_LIBRARY
  NAMES mlxc
  HINTS "${CMAKE_PREFIX_PATH}/lib"
  REQUIRED)

add_library(mlxrs_gap_shim STATIC src/gap_shim.cpp)

target_include_directories(
  mlxrs_gap_shim
  PUBLIC $<BUILD_INTERFACE:${CMAKE_CURRENT_LIST_DIR}/include>
         $<INSTALL_INTERFACE:include>)

# mlx brings the C++ headers; mlxc's installed headers come from the same
# prefix this shim installs into (mlx/c/*.h already live there).
target_link_libraries(mlxrs_gap_shim PUBLIC mlx ${MLXC_LIBRARY})

set_property(TARGET mlxrs_gap_shim PROPERTY POSITION_INDEPENDENT_CODE ON)

include(GNUInstallDirs)

install(
  TARGETS mlxrs_gap_shim
  ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR})

install(
  FILES include/mlxrs/gap_shim.h
  DESTINATION ${CMAKE_INSTALL_INCLUDEDIR}/mlxrs)
