// Made by Claude
// kgame_cxx.h - door 2's C++ half: what the cxx bridge in src/door_cpp.rs is allowed to call. Values are copied
// across once each way (Rust slice -> std::vector in, std::vector -> Rust slice out); nothing keeps Rust memory.
#pragma once
#include <cstddef>
#include <cstdint>
#include <memory>

#include "kgame_core.h"
#include "rust/cxx.h"

namespace kgame {

struct Args { std::vector<Val> items; };
struct Outs { std::vector<Val> items; };

std::unique_ptr<Args> new_args();
void add_floats(Args& a, rust::Slice<const double> v, bool atom);
void add_ints(Args& a, rust::Slice<const int64_t> v, bool atom);
std::unique_ptr<Outs> call(int32_t fnum, const Args& a);   // a C++ exception becomes a Kona error
size_t out_count(const Outs& o);
bool out_is_ints(const Outs& o, size_t k);
bool out_is_atom(const Outs& o, size_t k);
rust::Slice<const double> out_floats(const Outs& o, size_t k);
rust::Slice<const int64_t> out_ints(const Outs& o, size_t k);

}  // namespace kgame
// Made by Claude
