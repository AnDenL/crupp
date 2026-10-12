module;

#include <concepts>
#include <cstddef>
#include <type_traits>

export module HeavyMeta;

export namespace meta {

template <typename T>
concept Number = std::integral<T> || std::floating_point<T>;

template <typename... Ts>
struct TypeList {
    static constexpr std::size_t size = sizeof...(Ts);
};

template <int N, int Branch = 0>
struct TemplateTree {
    static constexpr long long value =
        TemplateTree<N - 1, Branch * 2 + 1>::value +
        TemplateTree<N - 1, Branch * 2 + 2>::value;
};

template <int Branch>
struct TemplateTree<0, Branch> {
    static constexpr long long value = Branch * 7 + 3;
};

template <std::size_t Dim>
struct ConstMatrix {
    long long data[Dim][Dim]{};

    constexpr ConstMatrix() {
        for (std::size_t i = 0; i < Dim; ++i) {
            for (std::size_t j = 0; j < Dim; ++j) {
                data[i][j] = static_cast<long long>((i + 2) * (j + 3));
            }
        }
    }

    constexpr long long trace() const {
        long long sum = 0;
        for (std::size_t i = 0; i < Dim; ++i) {
            sum += data[i][i];
        }
        return sum;
    }
};

template <Number T, T A, T B>
constexpr T add_numbers() {
    return A + B;
}

template <int Depth>
constexpr long long calculate_tree() {
    return TemplateTree<Depth>::value;
}

template <std::size_t Dim>
constexpr long long calculate_trace() {
    constexpr ConstMatrix<Dim> m{};
    return m.trace();
}

}
