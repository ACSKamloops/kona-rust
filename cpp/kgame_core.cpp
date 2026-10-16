// Made by Claude
// kgame_core.cpp - the game library itself. Plain C++17, no Rust or socket code in here.
#include "kgame_core.h"

#include <cmath>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>

namespace kgame {

namespace {

struct World {
    std::vector<double> x, z, vx, vz;
};

std::mutex worlds_lock;
std::vector<std::unique_ptr<World>> worlds;   // handle = index; freed slots stay empty

const std::vector<double>& floats(const std::vector<Val>& a, size_t k, const char* what) {
    if (k >= a.size() || a[k].ints) throw std::runtime_error(std::string("expected a float list for ") + what);
    return a[k].f;
}

int64_t one_int(const std::vector<Val>& a, size_t k, const char* what) {
    if (k >= a.size() || !a[k].ints || a[k].i.size() != 1) throw std::runtime_error(std::string("expected one int for ") + what);
    return a[k].i[0];
}

double one_float(const std::vector<Val>& a, size_t k, const char* what) {
    if (k >= a.size() || a[k].ints || a[k].f.size() != 1) throw std::runtime_error(std::string("expected one float for ") + what);
    return a[k].f[0];
}

Val float_atom(double v) { Val r; r.atom = true; r.f.push_back(v); return r; }
Val int_atom(int64_t v) { Val r; r.ints = true; r.atom = true; r.i.push_back(v); return r; }
Val float_list(std::vector<double> v) { Val r; r.f = std::move(v); return r; }

World& world(int64_t h) {
    if (h < 0 || static_cast<size_t>(h) >= worlds.size() || !worlds[h]) throw std::runtime_error("no world with that handle");
    return *worlds[h];
}

}  // namespace

std::vector<Val> call_core(int32_t fn, const std::vector<Val>& args) {
    switch (fn) {
    case ECHO:
        return args;
    case SUM: {
        const auto& v = floats(args, 0, "sum");
        double s = 0;
        for (double d : v) s += d;
        return {float_atom(s)};
    }
    case MOVE: {
        const auto& p = floats(args, 0, "pos");
        const auto& v = floats(args, 1, "vel");
        double dt = one_float(args, 2, "dt");
        if (p.size() != v.size()) throw std::runtime_error("pos and vel are different lengths");
        std::vector<double> out(p.size());
        for (size_t k = 0; k < p.size(); k++) out[k] = p[k] + v[k] * dt;
        return {float_list(std::move(out))};
    }
    case WORLD: {
        int64_t n = one_int(args, 0, "world size");
        if (n < 0 || n > 100000000) throw std::runtime_error("world size out of range");
        auto w = std::make_unique<World>();
        w->x.resize(n); w->z.resize(n); w->vx.resize(n); w->vz.resize(n);
        uint64_t s = 42;   // same start every time, so both doors build identical worlds
        auto rnd = [&s] { s = s * 6364136223846793005ULL + 1442695040888963407ULL; return (s >> 11) * (1.0 / 9007199254740992.0); };
        for (int64_t k = 0; k < n; k++) {
            w->x[k] = 100 * rnd(); w->z[k] = 100 * rnd();
            double a = 6.283185307179586 * rnd(), sp = 1 + 4 * rnd();
            w->vx[k] = sp * std::cos(a); w->vz[k] = sp * std::sin(a);
        }
        std::lock_guard<std::mutex> g(worlds_lock);
        worlds.push_back(std::move(w));
        return {int_atom(static_cast<int64_t>(worlds.size() - 1))};
    }
    case STEP: {
        int64_t h = one_int(args, 0, "handle");
        double dt = one_float(args, 1, "dt");
        std::lock_guard<std::mutex> g(worlds_lock);
        World& w = world(h);
        size_t n = w.x.size();
        for (size_t k = 0; k < n; k++) {   // move, bounce off the walls of a 100 x 100 room
            double nx = w.x[k] + w.vx[k] * dt, nz = w.z[k] + w.vz[k] * dt;
            if (nx < 0 || nx > 100) { w.vx[k] = -w.vx[k]; nx = w.x[k]; }
            if (nz < 0 || nz > 100) { w.vz[k] = -w.vz[k]; nz = w.z[k]; }
            w.x[k] = nx; w.z[k] = nz;
        }
        return {int_atom(static_cast<int64_t>(n))};
    }
    case READ: {
        int64_t h = one_int(args, 0, "handle");
        std::lock_guard<std::mutex> g(worlds_lock);
        World& w = world(h);
        return {float_list(w.x), float_list(w.z)};
    }
    case FREE: {
        int64_t h = one_int(args, 0, "handle");
        std::lock_guard<std::mutex> g(worlds_lock);
        world(h);
        worlds[h].reset();
        return {int_atom(1)};
    }
    default:
        throw std::runtime_error("no function " + std::to_string(fn) + " in the game library");
    }
}

}  // namespace kgame
// Made by Claude
