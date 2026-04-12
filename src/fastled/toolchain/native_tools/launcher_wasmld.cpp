// clang-tool-chain native wasm-ld launcher
// Replaces the Python wasm-ld wrapper with near-zero startup overhead.
// wasm-ld is a native binary — this launcher bypasses Python entirely.
//
// Strategy: On first run (cache miss), invoke Python ONCE to discover the
// wasm-ld binary path via the clang_tool_chain package (which handles
// downloading and installation). Cache the result. All subsequent runs
// read the cache and exec wasm-ld directly — zero Python/Node overhead.
//
// Single-file C++17, no external dependencies beyond OS APIs and standard library.
//
// Build: clang++ -O3 -std=c++17 -o ctc-wasm-ld launcher_wasmld.cpp
//   Linux:   add -static-libstdc++ -static-libgcc -lpthread
//   Windows: add -static-libstdc++ -static-libgcc

#include <cctype>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <direct.h>
#include <io.h>
#include <process.h>
#include <windows.h>
#else
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>
#endif

// ============================================================================
// Section 0: Constants
// ============================================================================

static constexpr const char* CTC_TAG = "[ctc-wasm-ld] ";
static constexpr const char* CACHE_FILENAME = ".ctc-wasmld-cache";

#ifdef _WIN32
static constexpr char PATH_LIST_SEP = ';';
#else
static constexpr char PATH_LIST_SEP = ':';
#endif

// ============================================================================
// Section 1: Platform Abstraction
// ============================================================================

enum class Platform { Windows, Linux, Darwin };
enum class Arch { X86_64, ARM64 };

static Platform get_platform() {
#ifdef _WIN32
    return Platform::Windows;
#elif defined(__APPLE__)
    return Platform::Darwin;
#elif defined(__linux__)
    return Platform::Linux;
#else
#error "Unsupported platform"
#endif
}

static Arch get_arch() {
#if defined(__x86_64__) || defined(_M_X64)
    return Arch::X86_64;
#elif defined(__aarch64__) || defined(_M_ARM64)
    return Arch::ARM64;
#else
#error "Unsupported architecture"
#endif
}

static const char* platform_str(Platform p) {
    switch (p) {
    case Platform::Windows: return "win";
    case Platform::Linux: return "linux";
    case Platform::Darwin: return "darwin";
    }
    return "unknown";
}

static const char* arch_str(Arch a) {
    switch (a) {
    case Arch::X86_64: return "x86_64";
    case Arch::ARM64: return "arm64";
    }
    return "unknown";
}

static char path_sep() {
#ifdef _WIN32
    return '\\';
#else
    return '/';
#endif
}

static std::string path_join(const std::string& a, const std::string& b) {
    if (a.empty()) return b;
    if (b.empty()) return a;
    char last = a.back();
    if (last == '/' || last == '\\') return a + b;
    return a + path_sep() + b;
}

static bool path_exists(const std::string& path) {
#ifdef _WIN32
    DWORD attr = GetFileAttributesA(path.c_str());
    return attr != INVALID_FILE_ATTRIBUTES;
#else
    struct stat st;
    return stat(path.c_str(), &st) == 0;
#endif
}

static std::string get_home_dir() {
#ifdef _WIN32
    const char* profile = getenv("USERPROFILE");
    if (profile && profile[0]) return profile;
    const char* homedrive = getenv("HOMEDRIVE");
    const char* homepath = getenv("HOMEPATH");
    if (homedrive && homepath) return std::string(homedrive) + homepath;
    return "C:\\Users\\Default";
#else
    const char* home = getenv("HOME");
    if (home && home[0]) return home;
    return "/tmp";
#endif
}

static std::string get_env(const char* name) {
    const char* val = getenv(name);
    return val ? val : "";
}

static bool env_is_truthy(const char* name) {
    std::string val = get_env(name);
    return val == "1" || val == "true" || val == "yes";
}

static std::string read_file(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    if (!f) return "";
    std::ostringstream ss;
    ss << f.rdbuf();
    return ss.str();
}

static bool write_file_atomic(const std::string& path, const std::string& content) {
    std::string tmp = path + ".tmp." + std::to_string(
#ifdef _WIN32
        (int)GetCurrentProcessId()
#else
        (int)getpid()
#endif
    );
    {
        std::ofstream f(tmp, std::ios::binary);
        if (!f) return false;
        f.write(content.data(), (std::streamsize)content.size());
        if (!f) { std::remove(tmp.c_str()); return false; }
    }
#ifdef _WIN32
    if (!MoveFileExA(tmp.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING)) {
        DeleteFileA(path.c_str());
        if (!MoveFileA(tmp.c_str(), path.c_str())) {
            std::remove(tmp.c_str());
            return false;
        }
    }
#else
    if (rename(tmp.c_str(), path.c_str()) != 0) {
        std::remove(tmp.c_str());
        return false;
    }
#endif
    return true;
}

// ============================================================================
// Section 2: Find executable in PATH
// ============================================================================

static std::string find_in_path(const char* name) {
    std::string path_env = get_env("PATH");
    if (path_env.empty()) return "";

    std::istringstream ss(path_env);
    std::string dir;
    while (std::getline(ss, dir, PATH_LIST_SEP)) {
        if (dir.empty()) continue;
        std::string candidate = path_join(dir, name);
        if (path_exists(candidate)) return candidate;
    }
    return "";
}

static std::string find_python() {
#ifdef _WIN32
    std::string p = find_in_path("python.exe");
    if (!p.empty()) return p;
    p = find_in_path("python3.exe");
    if (!p.empty()) return p;
    p = find_in_path("py.exe");
    if (!p.empty()) return p;
#else
    std::string p = find_in_path("python3");
    if (!p.empty()) return p;
    p = find_in_path("python");
    if (!p.empty()) return p;
#endif
    return "";
}

// ============================================================================
// Section 3: Cache (single key: wasm_ld_path)
// ============================================================================

struct WasmLdCache {
    std::string wasm_ld_path;

    bool is_valid() const {
        return !wasm_ld_path.empty() && path_exists(wasm_ld_path);
    }
};

static WasmLdCache read_cache(const std::string& cache_path) {
    WasmLdCache cache;
    std::string content = read_file(cache_path);
    if (content.empty()) return cache;

    std::istringstream stream(content);
    std::string line;
    while (std::getline(stream, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        size_t eq = line.find('=');
        if (eq == std::string::npos) continue;
        std::string key = line.substr(0, eq);
        std::string val = line.substr(eq + 1);
        if (key == "wasm_ld_path") cache.wasm_ld_path = val;
    }
    return cache;
}

// ============================================================================
// Section 4: One-Shot Python Discovery
// ============================================================================

static std::string run_capture(const std::string& cmd) {
    std::string result;
#ifdef _WIN32
    // _popen goes through cmd.exe /c which strips the first and last quotes
    // if the command starts with a quote. Wrap in extra quotes so cmd.exe
    // strips the outer pair and leaves the inner quotes intact.
    std::string wrapped = "\"" + cmd + "\"";
    FILE* pipe = _popen(wrapped.c_str(), "r");
#else
    FILE* pipe = popen(cmd.c_str(), "r");
#endif
    if (!pipe) return "";
    char buf[4096];
    while (fgets(buf, sizeof(buf), pipe)) {
        result += buf;
    }
#ifdef _WIN32
    int rc = _pclose(pipe);
#else
    int rc = pclose(pipe);
#endif
    if (rc != 0) return "";
    return result;
}

// Python one-liner: discovers wasm-ld path via clang_tool_chain.
// This triggers emscripten download/install if needed.
// Avoid f-strings — cmd.exe mangles \" inside -c "..." on Windows.
static const char* DISCOVERY_SCRIPT =
    "from clang_tool_chain.execution.emscripten import find_emscripten_wasm_ld_binary; "
    "print('wasm_ld_path=' + str(find_emscripten_wasm_ld_binary()))";

static WasmLdCache discover_via_python(const std::string& cache_path) {
    std::string python = find_python();
    if (python.empty()) {
        fprintf(stderr, "%sPython not found in PATH. Needed for one-time path discovery.\n", CTC_TAG);
        exit(1);
    }

    fprintf(stderr, "%sFirst run — discovering wasm-ld path via Python (one-time)...\n", CTC_TAG);

    std::string cmd = "\"" + python + "\" -c \"" + DISCOVERY_SCRIPT + "\"";
    std::string output = run_capture(cmd);

    if (output.empty()) {
        fprintf(stderr, "%sDiscovery failed. Is clang-tool-chain installed?\n", CTC_TAG);
        fprintf(stderr, "%sTry: pip install clang-tool-chain && clang-tool-chain install emscripten\n", CTC_TAG);
        exit(1);
    }

    // Parse
    WasmLdCache cache;
    std::istringstream stream(output);
    std::string line;
    while (std::getline(stream, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        size_t eq = line.find('=');
        if (eq == std::string::npos) continue;
        std::string key = line.substr(0, eq);
        std::string val = line.substr(eq + 1);
        if (key == "wasm_ld_path") cache.wasm_ld_path = val;
    }

    if (cache.is_valid()) {
        write_file_atomic(cache_path, output);
        fprintf(stderr, "%sPath cached. Subsequent runs will be instant.\n", CTC_TAG);
    } else {
        fprintf(stderr, "%sDiscovery returned invalid wasm-ld path. Output:\n%s\n", CTC_TAG, output.c_str());
        exit(1);
    }

    return cache;
}

// ============================================================================
// Section 5: Process Execution
// ============================================================================

static void print_command(const std::vector<std::string>& cmd) {
    for (size_t i = 0; i < cmd.size(); i++) {
        if (i > 0) printf(" ");
        bool needs_quote = cmd[i].find(' ') != std::string::npos ||
                           cmd[i].find('\t') != std::string::npos;
        if (needs_quote) printf("\"");
        printf("%s", cmd[i].c_str());
        if (needs_quote) printf("\"");
    }
    printf("\n");
}

#ifdef _WIN32
static std::string win_quote_arg(const std::string& arg) {
    bool needs_quote = arg.empty() || arg.find(' ') != std::string::npos ||
                       arg.find('\t') != std::string::npos ||
                       arg.find('"') != std::string::npos;
    if (!needs_quote) return arg;
    std::string result = "\"";
    size_t num_bs = 0;
    for (size_t j = 0; j < arg.size(); j++) {
        if (arg[j] == '\\') {
            num_bs++;
        } else if (arg[j] == '"') {
            result.append(num_bs * 2 + 1, '\\');
            result += '"';
            num_bs = 0;
        } else {
            result.append(num_bs, '\\');
            result += arg[j];
            num_bs = 0;
        }
    }
    result.append(num_bs * 2, '\\');
    result += '"';
    return result;
}

static int create_process_and_wait(const std::vector<std::string>& cmd) {
    std::string cmdline;
    for (size_t i = 0; i < cmd.size(); i++) {
        if (i > 0) cmdline += ' ';
        cmdline += win_quote_arg(cmd[i]);
    }
    STARTUPINFOA si = {};
    si.cb = sizeof(si);
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    si.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
    si.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    si.dwFlags = STARTF_USESTDHANDLES;
    PROCESS_INFORMATION pi = {};
    std::vector<char> buf(cmdline.begin(), cmdline.end());
    buf.push_back('\0');
    if (!CreateProcessA(nullptr, buf.data(), nullptr, nullptr, TRUE,
                        0, nullptr, nullptr, &si, &pi)) {
        fprintf(stderr, "%sFailed to create process: %lu\n", CTC_TAG, GetLastError());
        return 1;
    }
    WaitForSingleObject(pi.hProcess, INFINITE);
    DWORD exit_code = 1;
    GetExitCodeProcess(pi.hProcess, &exit_code);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    return (int)exit_code;
}
#endif

[[noreturn]] static void exec_process(const std::vector<std::string>& cmd) {
#ifdef _WIN32
    // Use CreateProcess instead of _execv to properly inherit stdout/stderr
    // pipes (e.g. when invoked by Meson or other build systems).
    int rc = create_process_and_wait(cmd);
    exit(rc);
#else
    std::vector<const char*> argv_ptrs;
    for (const auto& s : cmd) argv_ptrs.push_back(s.c_str());
    argv_ptrs.push_back(nullptr);
    execv(cmd[0].c_str(), const_cast<char**>(argv_ptrs.data()));
    fprintf(stderr, "%sFailed to exec: %s\n", CTC_TAG, cmd[0].c_str());
    exit(127);
#endif
}

// ============================================================================
// Section 6: main()
// ============================================================================

int main(int argc, char* argv[]) {
    bool debug = env_is_truthy("CTC_DEBUG");

    Platform platform = get_platform();
    Arch arch = get_arch();

    if (debug) {
        fprintf(stderr, "[ctc-wasm-ld-debug] platform=%s arch=%s\n",
                platform_str(platform), arch_str(arch));
    }

    // --help
    bool dry_run = false;
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            printf("Usage: ctc-wasm-ld [options] [wasm-ld-args...]\n\n");
            printf("Native wasm-ld launcher — bypasses Python/Node startup overhead.\n\n");
            printf("Launcher flags:\n");
            printf("  --dry-run    Print the command that would be exec'd\n");
            printf("  --help, -h   Show this help\n\n");
            printf("All other arguments are passed directly to wasm-ld.\n");
            printf("Environment: CTC_DEBUG=1 for debug output.\n");
            return 0;
        }
        if (strcmp(argv[i], "--dry-run") == 0) {
            dry_run = true;
        }
    }

    // 1. Resolve cache path
    std::string home = get_home_dir();
    std::string install_dir = path_join(home, ".clang-tool-chain");
    install_dir = path_join(install_dir, "emscripten");
    install_dir = path_join(install_dir, platform_str(platform));
    install_dir = path_join(install_dir, arch_str(arch));
    std::string cache_path = path_join(install_dir, CACHE_FILENAME);

    // 2. Read cache or discover via Python (one-time)
    WasmLdCache cache = read_cache(cache_path);
    if (!cache.is_valid()) {
        cache = discover_via_python(cache_path);
    }

    if (debug) {
        fprintf(stderr, "[ctc-wasm-ld-debug] wasm_ld_path=%s\n", cache.wasm_ld_path.c_str());
    }

    // 3. Build command: wasm-ld [user args...]
    std::vector<std::string> cmd;
    cmd.push_back(cache.wasm_ld_path);
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--dry-run") == 0) continue;  // strip launcher flag
        cmd.push_back(argv[i]);
    }

    // 4. Exec (replaces this process — no Python, no Node, pure native)
    if (dry_run) { print_command(cmd); return 0; }
    exec_process(cmd);
}
