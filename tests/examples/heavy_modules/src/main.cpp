#include <iostream>

import HeavyMeta;

int main() {
    constexpr auto sum = meta::add_numbers<int, 40, 2>();

    constexpr auto tree_res = meta::calculate_tree<12>();

    constexpr auto trace_res = meta::calculate_trace<40>();

    std::cout << "Sum: " << sum << "\n";
    std::cout << "Tree AST Result: " << tree_res << "\n";
    std::cout << "Matrix Trace: " << trace_res << "\n";

    return 0;
}
