#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

typedef struct {
    const char *name;
    const char *expr;
} Case;

typedef struct {
    char *out;
    size_t len;
    int status;
} RunResult;

static void append_bytes(RunResult *r, const char *buf, size_t n) {
    char *next = realloc(r->out, r->len + n + 1);
    if (!next) {
        perror("realloc");
        exit(2);
    }
    r->out = next;
    memcpy(r->out + r->len, buf, n);
    r->len += n;
    r->out[r->len] = '\0';
}

static RunResult run_k(const char *program, const char *expr) {
    int pipefd[2];
    if (pipe(pipefd) != 0) {
        perror("pipe");
        exit(2);
    }

    pid_t pid = fork();
    if (pid < 0) {
        perror("fork");
        exit(2);
    }

    if (pid == 0) {
        close(pipefd[0]);
        dup2(pipefd[1], STDOUT_FILENO);
        dup2(pipefd[1], STDERR_FILENO);
        close(pipefd[1]);
        execl(program, program, "-x", expr, (char *)NULL);
        fprintf(stderr, "exec failed for %s: %s\n", program, strerror(errno));
        _exit(127);
    }

    close(pipefd[1]);
    RunResult result = {0};
    char buf[4096];
    for (;;) {
        ssize_t n = read(pipefd[0], buf, sizeof(buf));
        if (n > 0) {
            append_bytes(&result, buf, (size_t)n);
            continue;
        }
        if (n == 0) {
            break;
        }
        if (errno == EINTR) {
            continue;
        }
        perror("read");
        exit(2);
    }
    close(pipefd[0]);

    int status = 0;
    while (waitpid(pid, &status, 0) < 0) {
        if (errno == EINTR) {
            continue;
        }
        perror("waitpid");
        exit(2);
    }

    if (WIFEXITED(status)) {
        result.status = WEXITSTATUS(status);
    } else if (WIFSIGNALED(status)) {
        result.status = -WTERMSIG(status);
    } else {
        result.status = 999;
    }

    if (!result.out) {
        result.out = calloc(1, 1);
        if (!result.out) {
            perror("calloc");
            exit(2);
        }
    }
    return result;
}

static int same_result(const RunResult *a, const RunResult *b) {
    return a->status == b->status && strcmp(a->out, b->out) == 0;
}

static void print_block(const char *label, const RunResult *r) {
    printf("%s status=%d output:\n", label, r->status);
    if (r->len == 0) {
        printf("<empty>\n");
    } else {
        fputs(r->out, stdout);
        if (r->out[r->len - 1] != '\n') {
            putchar('\n');
        }
    }
}

int main(void) {
    const char *c_kona = getenv("C_KONA");
    const char *rust_kona = getenv("RUST_KONA");
    if (!c_kona) {
        c_kona = "../kona-master3/k";
    }
    if (!rust_kona) {
        rust_kona = "./target/debug/kona";
    }

    const Case cases[] = {
        {"smoke_add", "1+1"},
        {"smoke_vector_add", "1 2+3 4"},
        {"smoke_take", "2#1 2 3"},
        {"smoke_index", "(`a`b)[1]"},
        {"smoke_lambda", "{x+y}[1;2]"},
        {"at_star_index", "a:2 3;a@*:0"},
        {"trap_amend_valence", "@[.:;\"a: 1 1 1; a/[0] +: 10\";:]"},
        {"trap_limit", "@[.:;\"!0I\";:]"},
        {"missing_conditional_branch", "\"a\",:[1;;\"b\"]"},
        {"symbol_match_cast", "`one `two  _sm $`two"},
        {"dict_index", "a:.((`b;1);(`c;2)); `a[`b`c]"},
        {"backslash_assignment", "a:1; \\b:2; c:3"},
        {"matrix_backslash_index", "m:3 4#!12; m\\[1;2]"},
        {"matrix_backslash_scalar", "m:3 4#!12; m\\2"},
        {"matrix_left_backslash", "m:3 4#!12; 1 m\\2"},
        {"each_projection_mul", "_mul[(3 5;1 3)]/[;1 0] 5"},
        {"trap_identity_string", ".[{x};\"2+2\";:]"},
        {"trap_eval_string", ".[{. x};\"2+2\";:]"},
        {"root_finder_a", "?[{(x^2)+(-x)+(-1)};0;1]"},
        {"root_finder_b", "?[{(x^2)+(-x)+(-1)};0;.25]"},
        {"sort_special_floats", "v@<v:0. 0i -0i 0n"},
        {"grade_mixed_strings_a", "<(\"aaa\";\"bb\";,\"c\";\"d\")"},
        {"grade_mixed_strings_b", "<(\"aaa\";\"bb\";\"c\";,\"d\")"},
        {"grade_mixed_strings_c", ">(\"ab\";,\"a\";\"aba\";\"a\")"},
        {"grade_null_char", "<(\"\";,\"\\000\";\" \")"},
        {"k_context_eval", "c:2; .k[\"c+1\"]"},
        {"namespace_eval", "c:2; d.c:42; d[\"c+1\"]"},
        {"float_unique", "?2. 1.9999999999999"},
        {"float_group", "=2. 1.9999999999999"},
        {"float_find_int", "v:1.9999999999999 2.;v?2"},
        {"float_find_float", "v:1.9999999999999 2.;v?2."},
        {"hash_find_int", "v:1.9999999999999 2.;(v;_hash v)?2"},
        {"hash_find_float", "v:1.9999999999999 2.;(v;_hash v)?2."},
        {"over_empty", "0,/()"},
        {"nan_add_int_null", "0i+0N"},
        {"close_float_group", "=1.0 1.00000000000001"},
    };

    size_t total = sizeof(cases) / sizeof(cases[0]);
    size_t passed = 0;
    size_t mismatched = 0;
    size_t oracle_failed = 0;

    printf("C oracle:    %s\n", c_kona);
    printf("Rust target: %s\n\n", rust_kona);

    for (size_t i = 0; i < total; i++) {
        RunResult expected = run_k(c_kona, cases[i].expr);
        RunResult actual = run_k(rust_kona, cases[i].expr);

        if (expected.status != 0) {
            oracle_failed++;
            printf("ORACLE_FAIL %-24s %s\n", cases[i].name, cases[i].expr);
            print_block("  C", &expected);
            print_block("  Rust", &actual);
            putchar('\n');
        } else if (same_result(&expected, &actual)) {
            passed++;
            printf("PASS        %-24s %s\n", cases[i].name, cases[i].expr);
        } else {
            mismatched++;
            printf("FAIL        %-24s %s\n", cases[i].name, cases[i].expr);
            print_block("  C", &expected);
            print_block("  Rust", &actual);
            putchar('\n');
        }

        free(expected.out);
        free(actual.out);
    }

    printf("\nSummary: total=%zu pass=%zu fail=%zu oracle_fail=%zu\n",
           total, passed, mismatched, oracle_failed);
    return mismatched ? 1 : 0;
}
