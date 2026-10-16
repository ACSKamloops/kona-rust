// Made by Claude
// kgame_cxx.cpp - door 2: the thin layer between the cxx bridge and the game library.
#include "kona/cpp/kgame_cxx.h"

namespace kgame {

std::unique_ptr<Args> new_args() { return std::make_unique<Args>(); }

void add_floats(Args& a, rust::Slice<const double> v, bool atom) {
    Val x;
    x.atom = atom;
    x.f.assign(v.data(), v.data() + v.size());
    a.items.push_back(std::move(x));
}

void add_ints(Args& a, rust::Slice<const int64_t> v, bool atom) {
    Val x;
    x.ints = true;
    x.atom = atom;
    x.i.assign(v.data(), v.data() + v.size());
    a.items.push_back(std::move(x));
}

std::unique_ptr<Outs> call(int32_t fnum, const Args& a) {
    auto o = std::make_unique<Outs>();
    o->items = call_core(fnum, a.items);
    return o;
}

size_t out_count(const Outs& o) { return o.items.size(); }

// these four never throw (an exception here would end the program, not become a Kona error):
// a bad index answers "empty", and the Rust side only asks for k < out_count anyway
static const Val empty_val;
static const Val& item(const Outs& o, size_t k) { return k < o.items.size() ? o.items[k] : empty_val; }

bool out_is_ints(const Outs& o, size_t k) { return item(o, k).ints; }
bool out_is_atom(const Outs& o, size_t k) { return item(o, k).atom; }
rust::Slice<const double> out_floats(const Outs& o, size_t k) {
    const Val& v = item(o, k);
    if (v.f.empty()) return {};
    return {v.f.data(), v.f.size()};
}
rust::Slice<const int64_t> out_ints(const Outs& o, size_t k) {
    const Val& v = item(o, k);
    if (v.i.empty()) return {};
    return {v.i.data(), v.i.size()};
}

}  // namespace kgame
// Made by Claude
