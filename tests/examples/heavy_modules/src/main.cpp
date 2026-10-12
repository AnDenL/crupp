#include <iostream>

import HeavyMeta;

int main() {
    // Інстанціювання шаблонів із модуля на боці клієнта
    constexpr auto sum = meta::add_numbers<int, 40, 2>();

    // Глибока шаблонна рекурсія (глибина 18 розгортається у 2^19 вузлів AST)
    constexpr auto tree_res = meta::calculate_tree<18>();

    // Constexpr трасування матриці 40x40
    constexpr auto trace_res = meta::calculate_trace<40>();

    std::cout << "Sum: " << sum << "\n";
    std::cout << "Tree AST Result: " << tree_res << "\n";
    std::cout << "Matrix Trace: " << trace_res << "\n";

    return 0;
}
