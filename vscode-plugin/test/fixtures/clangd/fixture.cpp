#include <stdint.h>
#include <stddef.h>

constexpr uint32_t pixels(size_t count) { return static_cast<uint32_t>(count * 3); }
int main() { return static_cast<int>(pixels(10)); }
