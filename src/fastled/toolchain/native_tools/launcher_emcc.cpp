// clang-tool-chain native Emscripten launcher
// Replaces the Python emcc/em++ wrapper with near-zero startup overhead.
//
// Strategy (from FastLED's EMCC_FAST pattern):
//   emcc is just a wrapper around clang (compile) and wasm-ld (link).
//
//   Three execution tiers (fastest to slowest):
//
//   1. USER TEMPLATE (--compile-commands / --link-args):
//      User provides a pre-built JSON/line-per-arg template containing the
//      full clang or wasm-ld command with {input}/{output} placeholders.
//      The launcher substitutes and execs the native binary directly.
//      Zero Python/Node overhead. User takes responsibility for correctness.
//
//   2. AUTO-CACHE (compile -c only):
//      On first compile: invoke emcc with EMCC_VERBOSE=1, parse the actual
//      clang command from stderr, templatize it, cache per flag-hash.
//      Subsequent compiles with same flags: call clang directly.
//
//   3. PYTHON FALLBACK:
//      For link, preprocess, or any uncached invocation: exec python emcc.py.
//
//   Capture modes (--capture-compile-commands / --capture-link-args):
//      Run emcc with EMCC_VERBOSE=1, parse the native command from stderr,
//      templatize it, and save to the specified file for later use with
//      --compile-commands / --link-args.
//
// Build: clang++ -O3 -std=c++17 -o ctc-emcc launcher_emcc.cpp
//   Linux:   add -static-libstdc++ -static-libgcc -lpthread
//   Windows: add -static-libstdc++ -static-libgcc

#include <algorithm>
#include <cctype>
#include <cstdint>
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

static constexpr const char* CTC_TAG = "[ctc-emcc] ";
static constexpr const char* PATHS_CACHE = ".ctc-emcc-paths";
static constexpr const char* ARGS_CACHE_DIR = ".ctc-emcc-args";

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
enum class EmccMode { C, CXX };

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

static bool is_directory(const std::string& path) {
#ifdef _WIN32
    DWORD attr = GetFileAttributesA(path.c_str());
    return attr != INVALID_FILE_ATTRIBUTES && (attr & FILE_ATTRIBUTE_DIRECTORY);
#else
    struct stat st;
    return stat(path.c_str(), &st) == 0 && S_ISDIR(st.st_mode);
#endif
}

static void make_directory(const std::string& path) {
#ifdef _WIN32
    CreateDirectoryA(path.c_str(), nullptr);
#else
    mkdir(path.c_str(), 0755);
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

static std::string get_exe_basename(const std::string& path) {
    size_t pos = path.find_last_of("/\\");
    std::string name = (pos == std::string::npos) ? path : path.substr(pos + 1);
    if (name.size() > 4) {
        std::string ext = name.substr(name.size() - 4);
        for (auto& c : ext) c = (char)tolower((unsigned char)c);
        if (ext == ".exe") name = name.substr(0, name.size() - 4);
    }
    return name;
}

static std::string get_extension(const std::string& path) {
    size_t dot = path.rfind('.');
    size_t sep = path.find_last_of("/\\");
    if (dot == std::string::npos || (sep != std::string::npos && dot < sep)) return "";
    std::string ext = path.substr(dot);
    for (auto& c : ext) c = (char)tolower((unsigned char)c);
    return ext;
}

static std::string to_lower(const std::string& s) {
    std::string r = s;
    for (auto& c : r) c = (char)tolower((unsigned char)c);
    return r;
}

static bool starts_with(const std::string& s, const char* prefix) {
    return s.compare(0, strlen(prefix), prefix) == 0;
}

static std::string get_env(const char* name) {
    const char* val = getenv(name);
    return val ? val : "";
}

static bool env_is_truthy(const char* name) {
    std::string val = get_env(name);
    return val == "1" || val == "true" || val == "yes";
}

static void set_env(const char* name, const std::string& value) {
#ifdef _WIN32
    SetEnvironmentVariableA(name, value.c_str());
    _putenv_s(name, value.c_str());
#else
    setenv(name, value.c_str(), 1);
#endif
}

static void unset_env(const char* name) {
#ifdef _WIN32
    SetEnvironmentVariableA(name, nullptr);
    std::string s = std::string(name) + "=";
    _putenv(s.c_str());
#else
    unsetenv(name);
#endif
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

static std::string get_temp_path() {
#ifdef _WIN32
    char buf[MAX_PATH];
    GetTempPathA(MAX_PATH, buf);
    return std::string(buf) + "ctc_emcc_" + std::to_string(GetCurrentProcessId()) + ".tmp";
#else
    return "/tmp/ctc_emcc_" + std::to_string(getpid()) + ".tmp";
#endif
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
// Section 3: Shell Tokenizer + JSON Parser
// ============================================================================

static std::vector<std::string> split_shell(const std::string& line) {
    std::vector<std::string> tokens;
    std::string current;
    bool in_single = false, in_double = false, escape = false;
    for (size_t i = 0; i < line.size(); i++) {
        char c = line[i];
        if (escape) { current += c; escape = false; continue; }
#ifdef _WIN32
        // On Windows, backslash is a path separator, not an escape character.
        // Only treat \ as escape when followed by a quote or another backslash
        // inside a double-quoted string.
        if (c == '\\' && !in_single && in_double && i + 1 < line.size()) {
            char next = line[i + 1];
            if (next == '"' || next == '\\') { escape = true; continue; }
        }
#else
        if (c == '\\' && !in_single) { escape = true; continue; }
#endif
        if (c == '\'' && !in_double) { in_single = !in_single; continue; }
        if (c == '"' && !in_single) { in_double = !in_double; continue; }
        if ((c == ' ' || c == '\t') && !in_single && !in_double) {
            if (!current.empty()) { tokens.push_back(current); current.clear(); }
            continue;
        }
        current += c;
    }
    if (!current.empty()) tokens.push_back(current);
    return tokens;
}

// Minimal JSON string array parser: ["arg1", "arg2", ...]
// Handles escaped quotes and backslashes inside strings.
static std::vector<std::string> parse_json_array(const std::string& content) {
    std::vector<std::string> result;
    // Find opening bracket
    size_t pos = content.find('[');
    if (pos == std::string::npos) return result;
    pos++;

    while (pos < content.size()) {
        // Skip whitespace and commas
        while (pos < content.size() && (content[pos] == ' ' || content[pos] == '\t' ||
               content[pos] == '\n' || content[pos] == '\r' || content[pos] == ','))
            pos++;
        if (pos >= content.size() || content[pos] == ']') break;

        // Expect opening quote
        if (content[pos] != '"') { pos++; continue; }
        pos++; // skip opening quote

        std::string s;
        while (pos < content.size() && content[pos] != '"') {
            if (content[pos] == '\\' && pos + 1 < content.size()) {
                char next = content[pos + 1];
                if (next == '"' || next == '\\' || next == '/') { s += next; pos += 2; }
                else if (next == 'n') { s += '\n'; pos += 2; }
                else if (next == 't') { s += '\t'; pos += 2; }
                else { s += content[pos]; pos++; }
            } else {
                s += content[pos]; pos++;
            }
        }
        if (pos < content.size()) pos++; // skip closing quote
        result.push_back(s);
    }
    return result;
}

// Read a template file. Auto-detects format:
//   - If starts with '[': JSON array of strings
//   - Otherwise: one arg per line
static std::vector<std::string> read_template_file(const std::string& path) {
    std::string content = read_file(path);
    if (content.empty()) return {};

    // Trim leading whitespace to detect format
    size_t first = content.find_first_not_of(" \t\n\r");
    if (first != std::string::npos && content[first] == '[') {
        return parse_json_array(content);
    }

    // Line-per-arg format
    std::vector<std::string> args;
    std::istringstream ss(content);
    std::string line;
    while (std::getline(ss, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (!line.empty()) args.push_back(line);
    }
    return args;
}

// ============================================================================
// Section 4: FNV-1a Hash
// ============================================================================

static std::string compute_hash(const std::vector<std::string>& parts) {
    uint64_t h = 0xcbf29ce484222325ULL;
    for (const auto& s : parts) {
        for (char c : s) {
            h ^= (uint64_t)(unsigned char)c;
            h *= 0x100000001b3ULL;
        }
        h ^= 0xff;
        h *= 0x100000001b3ULL;
    }
    char buf[17];
    snprintf(buf, sizeof(buf), "%016llx", (unsigned long long)h);
    return buf;
}

// ============================================================================
// Section 5: Paths Cache (one-shot Python discovery)
// ============================================================================

struct PathsCache {
    std::string emscripten_dir;
    std::string config_path;
    std::string bin_dir;
    std::string node_path;
    std::string python_path;
    std::string emcc_script;
    std::string empp_script;

    bool is_valid() const {
        return !emscripten_dir.empty() && !python_path.empty() &&
               !emcc_script.empty() && path_exists(emcc_script) &&
               path_exists(python_path);
    }
};

static PathsCache parse_paths_cache(const std::string& content) {
    PathsCache c;
    std::istringstream stream(content);
    std::string line;
    while (std::getline(stream, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        size_t eq = line.find('=');
        if (eq == std::string::npos) continue;
        std::string key = line.substr(0, eq);
        std::string val = line.substr(eq + 1);
        if (key == "emscripten_dir") c.emscripten_dir = val;
        else if (key == "config_path") c.config_path = val;
        else if (key == "bin_dir") c.bin_dir = val;
        else if (key == "node_path") c.node_path = val;
        else if (key == "python_path") c.python_path = val;
        else if (key == "emcc_script") c.emcc_script = val;
        else if (key == "empp_script") c.empp_script = val;
    }
    return c;
}

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
    while (fgets(buf, sizeof(buf), pipe)) result += buf;
#ifdef _WIN32
    int rc = _pclose(pipe);
#else
    int rc = pclose(pipe);
#endif
    if (rc != 0) return "";
    return result;
}

// Discovery script: avoid \" inside -c "..." — cmd.exe mangles them.
// Use str() + concatenation instead of f-strings with inner quotes.
static const char* DISCOVERY_SCRIPT =
    "import sys; "
    "from pathlib import Path; "
    "from clang_tool_chain.execution.emscripten import "
    "find_emscripten_tool, ensure_nodejs_available, get_platform_info; "
    "pn, ar = get_platform_info(); "
    "d = Path.home() / '.clang-tool-chain' / 'emscripten' / pn / ar; "
    "emcc = find_emscripten_tool('emcc'); "
    "node = ensure_nodejs_available(); "
    "print('emscripten_dir=' + str(d / 'emscripten')); "
    "print('config_path=' + str(d / '.emscripten')); "
    "print('bin_dir=' + str(d / 'bin')); "
    "print('node_path=' + str(node)); "
    "print('python_path=' + sys.executable); "
    "print('emcc_script=' + str(emcc)); "
    "empp = d / 'emscripten' / 'em++.py'; "
    "empp = str(empp) if empp.exists() else str(d / 'emscripten' / 'em++'); "
    "print('empp_script=' + empp)";

static PathsCache discover_paths(const std::string& cache_path) {
    std::string python = find_python();
    if (python.empty()) {
        fprintf(stderr, "%sPython not found. Install Python to use Emscripten.\n", CTC_TAG);
        exit(1);
    }
    fprintf(stderr, "%sFirst run — discovering Emscripten paths (one-time)...\n", CTC_TAG);
    std::string cmd = "\"" + python + "\" -c \"" + DISCOVERY_SCRIPT + "\"";
    std::string output = run_capture(cmd);
    if (output.empty()) {
        fprintf(stderr, "%sDiscovery failed. Try: pip install clang-tool-chain && "
                "clang-tool-chain install emscripten\n", CTC_TAG);
        exit(1);
    }
    PathsCache c = parse_paths_cache(output);
    if (c.is_valid()) {
        write_file_atomic(cache_path, output);
        fprintf(stderr, "%sPaths cached.\n", CTC_TAG);
    } else {
        fprintf(stderr, "%sDiscovery returned incomplete paths.\n%s\n", CTC_TAG, output.c_str());
        exit(1);
    }
    return c;
}

// ============================================================================
// Section 6: User Arg Parsing
// ============================================================================

static bool is_source_ext(const std::string& ext) {
    return ext == ".c" || ext == ".cpp" || ext == ".cc" || ext == ".cxx" ||
           ext == ".c++" || ext == ".m" || ext == ".mm" || ext == ".s" || ext == ".S";
}

static bool is_input_ext(const std::string& ext) {
    return is_source_ext(ext) || ext == ".o" || ext == ".obj" || ext == ".a" ||
           ext == ".so" || ext == ".wasm" || ext == ".bc" || ext == ".ll";
}

struct UserArgs {
    bool is_compile = false;
    bool dry_run = false;
    std::string input_file;
    std::string output_file;

    // User template fast-paths
    std::string compile_commands;           // --compile-commands=<file>
    std::string link_args;                  // --link-args=<file>

    // Capture mode: run emcc, save the native command to file
    std::string capture_compile_commands;   // --capture-compile-commands=<file>
    std::string capture_link_args;          // --capture-link-args=<file>

    std::vector<std::string> all;     // all user args (minus launcher flags)
    std::vector<std::string> flags;   // args minus file paths (for auto-cache key)
};

static UserArgs parse_user_args(int argc, char* argv[]) {
    UserArgs u;
    for (int i = 1; i < argc; i++) {
        std::string arg = argv[i];

        // --compile-commands= is 19 chars
        if (starts_with(arg, "--compile-commands=")) {
            u.compile_commands = arg.substr(19);
            continue;
        }
        // --link-args= is 12 chars
        if (starts_with(arg, "--link-args=")) {
            u.link_args = arg.substr(12);
            continue;
        }
        // --capture-compile-commands= is 27 chars
        if (starts_with(arg, "--capture-compile-commands=")) {
            u.capture_compile_commands = arg.substr(27);
            continue;
        }
        // --capture-link-args= is 20 chars
        if (starts_with(arg, "--capture-link-args=")) {
            u.capture_link_args = arg.substr(20);
            continue;
        }
        if (arg == "--dry-run") {
            u.dry_run = true;
            continue;
        }

        u.all.push_back(arg);

        if (arg == "-c") {
            u.is_compile = true;
            u.flags.push_back(arg);
            continue;
        }
        if (arg == "-o" && i + 1 < argc) {
            u.output_file = argv[i + 1];
            u.flags.push_back("-o");
            u.all.push_back(argv[++i]);
            continue;
        }
        std::string ext = get_extension(arg);
        if (is_input_ext(ext) && u.input_file.empty()) {
            u.input_file = arg;
            continue;
        }
        u.flags.push_back(arg);
    }
    return u;
}

// ============================================================================
// Section 7: Template Substitution
// ============================================================================

static void str_replace_all(std::string& s, const std::string& from, const std::string& to) {
    size_t pos = 0;
    while ((pos = s.find(from, pos)) != std::string::npos) {
        s.replace(pos, from.size(), to);
        pos += to.size();
    }
}

// Substitute {input} and {output} placeholders with actual file paths
static std::vector<std::string> apply_substitutions(
    const std::vector<std::string>& tmpl,
    const std::string& input_file,
    const std::string& output_file) {

    std::vector<std::string> result;
    for (const auto& t : tmpl) {
        std::string a = t;
        if (!input_file.empty()) str_replace_all(a, "{input}", input_file);
        if (!output_file.empty()) str_replace_all(a, "{output}", output_file);
        result.push_back(a);
    }
    return result;
}

// Replace actual file paths with {input}/{output} for caching
static std::vector<std::string> templatize(const std::vector<std::string>& args,
                                            const std::string& input_file,
                                            const std::string& output_file) {
    std::vector<std::string> tmpl;
    for (const auto& a : args) {
        std::string t = a;
        if (!input_file.empty()) str_replace_all(t, input_file, "{input}");
        if (!output_file.empty()) str_replace_all(t, output_file, "{output}");
        tmpl.push_back(t);
    }
    return tmpl;
}

// ============================================================================
// Section 8: Auto-Cache (one-arg-per-line files)
// ============================================================================

static std::vector<std::string> read_arg_template(const std::string& path) {
    std::vector<std::string> args;
    std::string content = read_file(path);
    if (content.empty()) return args;
    std::istringstream ss(content);
    std::string line;
    while (std::getline(ss, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (!line.empty()) args.push_back(line);
    }
    return args;
}

static void write_arg_template(const std::string& path, const std::vector<std::string>& args) {
    std::string content;
    for (const auto& a : args) content += a + "\n";
    write_file_atomic(path, content);
}

// ============================================================================
// Section 9: EMCC_VERBOSE Capture and Parse
// ============================================================================

static int run_emcc_verbose(const PathsCache& paths,
                             const std::vector<std::string>& user_args,
                             EmccMode mode,
                             std::string& stderr_out) {
    std::string tmpfile = get_temp_path();

    const std::string& script = (mode == EmccMode::CXX) ? paths.empp_script : paths.emcc_script;
    std::string cmd = "\"" + paths.python_path + "\" \"" + script + "\"";
    for (const auto& a : user_args) {
        if (a.find(' ') != std::string::npos || a.find('\t') != std::string::npos) {
            cmd += " \"" + a + "\"";
        } else {
            cmd += " " + a;
        }
    }
    cmd += " 2>\"" + tmpfile + "\"";

#ifdef _WIN32
    cmd = "\"" + cmd + "\"";  // cmd.exe double-quote wrapping
#endif

    set_env("EMCC_VERBOSE", "1");
    set_env("EM_FORCE_RESPONSE_FILES", "0");
    set_env("EMSCRIPTEN", paths.emscripten_dir);
    set_env("EMSCRIPTEN_ROOT", paths.emscripten_dir);
    set_env("EM_CONFIG", paths.config_path);

    std::string node_bin;
    {
        size_t pos = paths.node_path.find_last_of("/\\");
        node_bin = (pos != std::string::npos) ? paths.node_path.substr(0, pos) : ".";
    }
    std::string old_path = get_env("PATH");
    set_env("PATH", paths.bin_dir + PATH_LIST_SEP + node_bin + PATH_LIST_SEP + old_path);

    int rc = system(cmd.c_str());

    unset_env("EMCC_VERBOSE");
    unset_env("EM_FORCE_RESPONSE_FILES");
    set_env("PATH", old_path);

    stderr_out = read_file(tmpfile);
    std::remove(tmpfile.c_str());

#ifndef _WIN32
    rc = WIFEXITED(rc) ? WEXITSTATUS(rc) : 1;
#endif
    return rc;
}

static std::vector<std::string> parse_clang_command(const std::string& stderr_content) {
    std::istringstream ss(stderr_content);
    std::string line;
    while (std::getline(ss, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty()) continue;
        auto parts = split_shell(line);
        if (parts.empty()) continue;
        std::string exe = to_lower(parts[0]);
        if (exe.find("clang") != std::string::npos) {
            for (const auto& p : parts) {
                if (p == "-c") return parts;
            }
        }
    }
    return {};
}

static std::vector<std::string> parse_wasmld_command(const std::string& stderr_content) {
    std::istringstream ss(stderr_content);
    std::string line;
    while (std::getline(ss, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty()) continue;
        auto parts = split_shell(line);
        if (parts.empty()) continue;
        std::string exe = to_lower(parts[0]);
        if (exe.find("wasm-ld") != std::string::npos) return parts;
    }
    return {};
}

// Write a JSON string array to a file
static bool write_json_array(const std::string& path, const std::vector<std::string>& args) {
    std::string content = "[\n";
    for (size_t i = 0; i < args.size(); i++) {
        content += "  \"";
        for (char c : args[i]) {
            if (c == '"') content += "\\\"";
            else if (c == '\\') content += "\\\\";
            else content += c;
        }
        content += "\"";
        if (i + 1 < args.size()) content += ",";
        content += "\n";
    }
    content += "]\n";
    return write_file_atomic(path, content);
}

static void relay_stderr(const std::string& stderr_content) {
    std::istringstream ss(stderr_content);
    std::string line;
    while (std::getline(ss, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.empty()) continue;
        auto parts = split_shell(line);
        if (parts.empty()) { fprintf(stderr, "%s\n", line.c_str()); continue; }
        std::string exe = to_lower(parts[0]);
        // Skip verbose command lines
        if (exe.find("clang") != std::string::npos || exe.find("wasm-ld") != std::string::npos)
            continue;
        fprintf(stderr, "%s\n", line.c_str());
    }
}

// ============================================================================
// Section 10: Mode Detection
// ============================================================================

static EmccMode detect_mode(const char* argv0) {
    std::string name = get_exe_basename(argv0);
    for (auto& c : name) c = (char)tolower((unsigned char)c);
    if (name.find("++") != std::string::npos) return EmccMode::CXX;
    if (name.find("empp") != std::string::npos) return EmccMode::CXX;
    return EmccMode::C;
}

// ============================================================================
// Section 11: Process Execution
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
// Proper Windows command-line argument quoting (handles backslashes + quotes).
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

// CreateProcess with proper stdout/stderr pipe inheritance.
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
// Section 11b: Windows Path Normalization
// ============================================================================
// On Windows, build systems (especially Meson) may pass include paths with
// inconsistent slash forms: backslash in cpp_args but forward-slash in
// custom_target commands.  This causes clang's #pragma once to fail across
// PCH boundaries because it sees the same header resolved via different
// canonical paths (e.g. C:\foo\bar.h vs C:/foo/bar.h).
//
// Fix: normalize all path-carrying arguments to forward slashes before
// passing them to real clang.  Clang on Windows handles forward slashes
// natively, so this is always safe.

#ifdef _WIN32
static std::string normalize_slashes(const std::string& s) {
    std::string out = s;
    for (auto& c : out) {
        if (c == '\\') c = '/';
    }
    return out;
}

static void normalize_windows_paths(std::vector<std::string>& cmd) {
    for (size_t i = 0; i < cmd.size(); i++) {
        auto& arg = cmd[i];

        // -I<path> or -isystem<path> (concatenated, no space)
        if (starts_with(arg, "-I") && arg.size() > 2 && arg[2] != '=') {
            arg = "-I" + normalize_slashes(arg.substr(2));
        } else if (starts_with(arg, "-isystem") && arg.size() > 8) {
            arg = "-isystem" + normalize_slashes(arg.substr(8));
        }
        // --sysroot=<path>
        else if (starts_with(arg, "--sysroot=")) {
            arg = "--sysroot=" + normalize_slashes(arg.substr(10));
        }
        // Flags followed by a separate path argument:
        //   -I <path>, -isystem <path>, -include <path>,
        //   -include-pch <path>, -isysroot <path>,
        //   -o <path>, -MF <path>, -MQ <path>, -MT <path>
        else if ((arg == "-I" || arg == "-isystem" || arg == "-include" ||
                  arg == "-include-pch" || arg == "-isysroot" ||
                  arg == "-o" || arg == "-MF" || arg == "-MQ" || arg == "-MT")
                 && i + 1 < cmd.size()) {
            cmd[i + 1] = normalize_slashes(cmd[i + 1]);
            i++;  // skip the path we just normalized
        }
        // Source files and other bare paths (heuristic: contains backslash
        // and looks like a file path — has a dot-extension or starts with
        // a drive letter).
        else if (arg.find('\\') != std::string::npos &&
                 !starts_with(arg, "-") &&
                 (arg.find('.') != std::string::npos ||
                  (arg.size() >= 2 && arg[1] == ':'))) {
            arg = normalize_slashes(arg);
        }
    }
}
#endif

// ============================================================================
// Section 12: main()
// ============================================================================

int main(int argc, char* argv[]) {
    bool debug = env_is_truthy("CTC_DEBUG");

    EmccMode mode = detect_mode(argv[0]);
    Platform platform = get_platform();
    Arch arch = get_arch();

    if (debug) {
        fprintf(stderr, "[ctc-emcc-debug] argv[0]=%s mode=%s platform=%s/%s\n",
                argv[0], mode == EmccMode::CXX ? "em++" : "emcc",
                platform_str(platform), arch_str(arch));
    }

    // --help: print launcher usage
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            const char* name = (mode == EmccMode::CXX) ? "ctc-em++" : "ctc-emcc";
            printf("Usage: %s [launcher-flags] [emcc-args...]\n\n", name);
            printf("Native Emscripten launcher — bypasses Python/Node startup overhead.\n\n");
            printf("Launcher flags (consumed by the launcher, not passed to emcc):\n");
            printf("  --compile-commands=<file>          Use pre-cached clang compile template\n");
            printf("  --link-args=<file>                 Use pre-cached wasm-ld link template\n");
            printf("  --capture-compile-commands=<file>  Run emcc, save clang template to file\n");
            printf("  --capture-link-args=<file>         Run emcc, save wasm-ld template to file\n");
            printf("  --dry-run                          Print the command that would be exec'd\n");
            printf("  --help, -h                         Show this help\n\n");
            printf("Execution tiers (fastest to slowest):\n");
            printf("  1. User template   --compile-commands / --link-args (zero Python)\n");
            printf("  2. Auto-cache      compile -c only, zero Python after first run\n");
            printf("  3. Python fallback exec python emcc.py\n\n");
            printf("Template files: JSON array or one-arg-per-line with {input}/{output} placeholders.\n");
            printf("Environment: CTC_DEBUG=1 for debug output.\n");
            return 0;
        }
    }

    // Resolve install directory and cache paths
    std::string home = get_home_dir();
    std::string install_dir = path_join(home, ".clang-tool-chain");
    install_dir = path_join(install_dir, "emscripten");
    install_dir = path_join(install_dir, platform_str(platform));
    install_dir = path_join(install_dir, arch_str(arch));
    std::string paths_cache_file = path_join(install_dir, PATHS_CACHE);
    std::string args_cache_dir = path_join(install_dir, ARGS_CACHE_DIR);

    // Parse user args (strips launcher flags)
    UserArgs user = parse_user_args(argc, argv);

    // ---------------------------------------------------------------
    // TIER 1: USER TEMPLATE — fastest path, zero Python
    //
    //   --compile-commands=<file>   (compile mode, -c flag present)
    //   --link-args=<file>          (link mode, no -c flag)
    //
    // The template file (JSON array or one-arg-per-line) contains the
    // full clang or wasm-ld command with {input}/{output} placeholders.
    // The launcher substitutes and execs the native binary directly.
    // No Python, no Node.
    // ---------------------------------------------------------------

    if (!user.compile_commands.empty()) {
        auto tmpl = read_template_file(user.compile_commands);
        if (tmpl.empty()) {
            fprintf(stderr, "%sCould not read compile commands: %s\n",
                    CTC_TAG, user.compile_commands.c_str());
            return 1;
        }
        auto cmd = apply_substitutions(tmpl, user.input_file, user.output_file);
#ifdef _WIN32
        normalize_windows_paths(cmd);
#endif
        if (debug) {
            fprintf(stderr, "[ctc-emcc-debug] TEMPLATE compile: %s (%zu args)\n",
                    cmd[0].c_str(), cmd.size());
        }
        if (user.dry_run) { print_command(cmd); return 0; }
        exec_process(cmd);
    }

    if (!user.link_args.empty()) {
        auto tmpl = read_template_file(user.link_args);
        if (tmpl.empty()) {
            fprintf(stderr, "%sCould not read link args: %s\n",
                    CTC_TAG, user.link_args.c_str());
            return 1;
        }
        auto cmd = apply_substitutions(tmpl, user.input_file, user.output_file);
#ifdef _WIN32
        normalize_windows_paths(cmd);
#endif
        if (debug) {
            fprintf(stderr, "[ctc-emcc-debug] TEMPLATE link: %s (%zu args)\n",
                    cmd[0].c_str(), cmd.size());
        }
        if (user.dry_run) { print_command(cmd); return 0; }
        exec_process(cmd);
    }

    // ---------------------------------------------------------------
    // CAPTURE MODES — run emcc verbose, save template to file
    //
    //   --capture-compile-commands=<file>
    //   --capture-link-args=<file>
    //
    // Runs emcc normally but with EMCC_VERBOSE=1, parses the native
    // clang or wasm-ld command from stderr, templatizes it, and writes
    // the template to the specified file.
    // ---------------------------------------------------------------

    if (!user.capture_compile_commands.empty() || !user.capture_link_args.empty()) {
        // Need paths for emcc invocation
        PathsCache paths = parse_paths_cache(read_file(paths_cache_file));
        if (!paths.is_valid()) paths = discover_paths(paths_cache_file);

        std::string stderr_out;
        int rc = run_emcc_verbose(paths, user.all, mode, stderr_out);
        relay_stderr(stderr_out);

        if (!user.capture_compile_commands.empty()) {
            auto clang_cmd = parse_clang_command(stderr_out);
            if (clang_cmd.empty()) {
                fprintf(stderr, "%sCould not find clang command in emcc output.\n", CTC_TAG);
                fprintf(stderr, "%sMake sure you pass -c to trigger a compile.\n", CTC_TAG);
            } else {
                auto tmpl = templatize(clang_cmd, user.input_file, user.output_file);
                if (write_json_array(user.capture_compile_commands, tmpl)) {
                    fprintf(stderr, "%sSaved compile template (%zu args) to: %s\n",
                            CTC_TAG, tmpl.size(), user.capture_compile_commands.c_str());
                } else {
                    fprintf(stderr, "%sFailed to write: %s\n",
                            CTC_TAG, user.capture_compile_commands.c_str());
                }
            }
        }

        if (!user.capture_link_args.empty()) {
            auto ld_cmd = parse_wasmld_command(stderr_out);
            if (ld_cmd.empty()) {
                fprintf(stderr, "%sCould not find wasm-ld command in emcc output.\n", CTC_TAG);
                fprintf(stderr, "%sMake sure you are linking (no -c flag).\n", CTC_TAG);
            } else {
                auto tmpl = templatize(ld_cmd, user.input_file, user.output_file);
                if (write_json_array(user.capture_link_args, tmpl)) {
                    fprintf(stderr, "%sSaved link template (%zu args) to: %s\n",
                            CTC_TAG, tmpl.size(), user.capture_link_args.c_str());
                } else {
                    fprintf(stderr, "%sFailed to write: %s\n",
                            CTC_TAG, user.capture_link_args.c_str());
                }
            }
        }

        return rc;
    }

    // ---------------------------------------------------------------
    // TIER 2: AUTO-CACHE — compile mode only
    //
    // Cache key = hash of flag args (excludes file paths).
    // On cache hit: call clang directly, zero Python overhead.
    // ---------------------------------------------------------------

    if (user.is_compile && !user.input_file.empty()) {
        std::string hash = compute_hash(user.flags);
        std::string cached_file = path_join(args_cache_dir, hash + ".args");

        if (path_exists(cached_file)) {
            auto tmpl = read_arg_template(cached_file);
            if (!tmpl.empty()) {
                auto cmd = apply_substitutions(tmpl, user.input_file, user.output_file);
#ifdef _WIN32
                normalize_windows_paths(cmd);
#endif
                if (debug) {
                    fprintf(stderr, "[ctc-emcc-debug] AUTO-CACHE HIT: %s\n", cached_file.c_str());
                }
                if (user.dry_run) { print_command(cmd); return 0; }
                exec_process(cmd);
            }
        }

        if (debug) {
            fprintf(stderr, "[ctc-emcc-debug] AUTO-CACHE MISS: hash=%s\n", hash.c_str());
        }
    }

    // ---------------------------------------------------------------
    // TIER 3: PYTHON FALLBACK — discovery + emcc execution
    // ---------------------------------------------------------------

    PathsCache paths = parse_paths_cache(read_file(paths_cache_file));
    if (!paths.is_valid()) {
        paths = discover_paths(paths_cache_file);
    }

    // For compile mode: run emcc with EMCC_VERBOSE=1, cache clang args for next time
    if (user.is_compile && !user.input_file.empty()) {
        std::string stderr_out;
        int rc = run_emcc_verbose(paths, user.all, mode, stderr_out);
        relay_stderr(stderr_out);

        auto clang_cmd = parse_clang_command(stderr_out);
        if (!clang_cmd.empty()) {
            auto tmpl = templatize(clang_cmd, user.input_file, user.output_file);
            if (!is_directory(args_cache_dir)) make_directory(args_cache_dir);
            std::string hash = compute_hash(user.flags);
            write_arg_template(path_join(args_cache_dir, hash + ".args"), tmpl);
            if (debug) {
                fprintf(stderr, "[ctc-emcc-debug] Cached %zu clang args (hash=%s)\n",
                        tmpl.size(), hash.c_str());
            }
        }
        return rc;
    }

    // Non-compile: exec python emcc.py (link, preprocess, etc.)
    set_env("EMSCRIPTEN", paths.emscripten_dir);
    set_env("EMSCRIPTEN_ROOT", paths.emscripten_dir);
    set_env("EM_CONFIG", paths.config_path);
    std::string node_bin;
    {
        size_t pos = paths.node_path.find_last_of("/\\");
        node_bin = (pos != std::string::npos) ? paths.node_path.substr(0, pos) : ".";
    }
    std::string old_path = get_env("PATH");
    set_env("PATH", paths.bin_dir + PATH_LIST_SEP + node_bin + PATH_LIST_SEP + old_path);

    const std::string& script = (mode == EmccMode::CXX) ? paths.empp_script : paths.emcc_script;
    std::vector<std::string> cmd;
    cmd.push_back(paths.python_path);
    cmd.push_back(script);
    for (const auto& a : user.all) cmd.push_back(a);

    if (user.dry_run) { print_command(cmd); return 0; }
    exec_process(cmd);
}
