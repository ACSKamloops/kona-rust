// Made by Claude
// kgame_core.h - a small C++ "game library" for Kona's _sys door. The same code is reached two ways:
//   library 2: built into kona.exe (cxx bridge, src/door_cpp.rs + kgame_cxx.cpp)
//   library 3: running next door as kgame_server.exe (socket, src/door_net.rs + kgame_server.cpp)
// Like Martin Richards' BCPL sys(Sys_gl, fno, ...): one entry point, a function number, plain values in and out.
#pragma once
#include <cstdint>
#include <vector>

namespace kgame {

// one value crossing the door: a float list or an int list; atom = a single number, not a list
struct Val {
    bool ints = false;
    bool atom = false;
    std::vector<double> f;
    std::vector<int64_t> i;
};

// function numbers
enum Fn : int32_t {
    ECHO = 0,    // anything -> the same values back (measures the door itself)
    SUM = 1,     // float list -> its sum
    MOVE = 2,    // (pos; vel; dt) -> pos + vel*dt
    WORLD = 3,   // n -> handle of a new world of n particles that lives on the C++ side
    STEP = 4,    // (handle; dt) -> particle count, after moving and bouncing every particle in that world
    READ = 5,    // handle -> (x; z) positions
    FREE = 6,    // handle -> 1, world dropped
};

// throws std::runtime_error with a plain message when the call is wrong
std::vector<Val> call_core(int32_t fn, const std::vector<Val>& args);

}  // namespace kgame
// Made by Claude
