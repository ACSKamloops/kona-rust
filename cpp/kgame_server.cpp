// Made by Claude
// kgame_server.cpp - door 3: the game library running as its own program next to Kona. Listens on 127.0.0.1 only
// (nothing else on the network can reach it). Usage: kgame_server.exe [port]   (default 5990)
//
// Messages (little-endian), one request -> one reply on a kept-open connection:
//   request: int32 fn, int32 count, then per value: int32 tag, int64 n, n * 8 bytes
//   reply:   int32 0, int32 count, values as above      or      int32 1, int64 len, len bytes of error text
//   tag: bit 0 = ints (int64) else floats (double), bit 1 = atom (single number)
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <ws2tcpip.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <string>
#include <thread>

#include "kgame_core.h"

using kgame::Val;

static bool recv_all(SOCKET s, void* buf, size_t n) {
    char* p = static_cast<char*>(buf);
    while (n > 0) {
        int got = recv(s, p, n > (1 << 30) ? (1 << 30) : static_cast<int>(n), 0);
        if (got <= 0) return false;
        p += got;
        n -= static_cast<size_t>(got);
    }
    return true;
}

static bool send_all(SOCKET s, const void* buf, size_t n) {
    const char* p = static_cast<const char*>(buf);
    while (n > 0) {
        int sent = send(s, p, n > (1 << 30) ? (1 << 30) : static_cast<int>(n), 0);
        if (sent <= 0) return false;
        p += sent;
        n -= static_cast<size_t>(sent);
    }
    return true;
}

template <class T> static void put(std::string& b, T v) { b.append(reinterpret_cast<const char*>(&v), sizeof v); }

static void serve(SOCKET c) {
    int one = 1;
    setsockopt(c, IPPROTO_TCP, TCP_NODELAY, reinterpret_cast<const char*>(&one), sizeof one);
    std::string out;
    for (;;) {
        int32_t head[2];
        if (!recv_all(c, head, sizeof head)) break;
        if (head[1] < 0 || head[1] > 4096) break;
        std::vector<Val> args(static_cast<size_t>(head[1]));
        bool ok = true;
        for (Val& v : args) {
            int32_t tag; int64_t n;
            if (!recv_all(c, &tag, 4) || !recv_all(c, &n, 8) || n < 0 || n > (int64_t(1) << 31)) { ok = false; break; }
            v.ints = tag & 1;
            v.atom = (tag & 2) != 0;
            if (v.ints) { v.i.resize(static_cast<size_t>(n)); ok = n == 0 || recv_all(c, v.i.data(), n * 8); }
            else        { v.f.resize(static_cast<size_t>(n)); ok = n == 0 || recv_all(c, v.f.data(), n * 8); }
            if (!ok) break;
        }
        if (!ok) break;
        out.clear();
        try {
            std::vector<Val> r = kgame::call_core(head[0], args);
            put<int32_t>(out, 0);
            put<int32_t>(out, static_cast<int32_t>(r.size()));
            for (const Val& v : r) {
                int64_t n = static_cast<int64_t>(v.ints ? v.i.size() : v.f.size());
                put<int32_t>(out, (v.ints ? 1 : 0) | (v.atom ? 2 : 0));
                put<int64_t>(out, n);
                if (n) out.append(v.ints ? reinterpret_cast<const char*>(v.i.data()) : reinterpret_cast<const char*>(v.f.data()), n * 8);
            }
        } catch (const std::exception& e) {
            out.clear();
            put<int32_t>(out, 1);
            put<int64_t>(out, static_cast<int64_t>(std::strlen(e.what())));
            out.append(e.what());
        }
        if (!send_all(c, out.data(), out.size())) break;
    }
    closesocket(c);
}

int main(int argc, char** argv) {
    int port = argc > 1 ? std::atoi(argv[1]) : 5990;
    WSADATA wd;
    if (WSAStartup(MAKEWORD(2, 2), &wd) != 0) { std::fprintf(stderr, "no sockets\n"); return 1; }
    SOCKET l = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    sockaddr_in a{};
    a.sin_family = AF_INET;
    a.sin_port = htons(static_cast<u_short>(port));
    inet_pton(AF_INET, "127.0.0.1", &a.sin_addr);   // this PC only
    if (bind(l, reinterpret_cast<sockaddr*>(&a), sizeof a) != 0 || listen(l, 8) != 0) {
        std::fprintf(stderr, "kgame_server: port %d is busy\n", port);
        return 1;
    }
    std::printf("kgame_server on 127.0.0.1:%d\n", port);
    std::fflush(stdout);
    for (;;) {
        SOCKET c = accept(l, nullptr, nullptr);
        if (c == INVALID_SOCKET) continue;
        std::thread(serve, c).detach();
    }
}
// Made by Claude
