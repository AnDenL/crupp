use super::types::{CtEnum, CtStruct};

pub fn generate_comptime_header(
    structs: &[CtStruct],
    enums: &[CtEnum],
) -> String {
    let mut s = String::new();

    s.push_str(r#"#ifndef COMPTIME_H
#define COMPTIME_H

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <ctype.h>

typedef enum {
    TYPE_INT8,
    TYPE_INT16,
    TYPE_INT32,
    TYPE_INT64,
    TYPE_UINT8,
    TYPE_UINT16,
    TYPE_UINT32,
    TYPE_UINT64,
    TYPE_FLOAT,
    TYPE_DOUBLE,
    TYPE_BOOL,
    TYPE_CHAR,
    TYPE_STRING,
    TYPE_STRUCT,
    TYPE_POINTER,
    TYPE_ARRAY,
    TYPE_UNKNOWN
} TypeKind;

typedef struct ct_attr {
    const char* name;
    const char* value;
} ct_attr;

typedef struct ct_field {
    const char* name;
    const char* type;
    TypeKind kind;
    int is_pointer;
    int array_len;
    int attr_count;
    ct_attr attrs[16];
} ct_field;

typedef struct ct_struct {
    const char* name;
    int field_count;
    ct_field fields[64];
    int attr_count;
    ct_attr attrs[32];
} ct_struct;

typedef struct ct_variant {
    const char* name;
    int64_t value;
    int attr_count;
    ct_attr attrs[16];
} ct_variant;

typedef struct ct_enum {
    const char* name;
    int variant_count;
    ct_variant variants[128];
    int attr_count;
    ct_attr attrs[32];
} ct_enum;

"#);

    // Static data for structs
    s.push_str("static const ct_struct _CT_ALL_STRUCTS[] = {\n");
    for st in structs {
        s.push_str(&format!(
            "    {{\n        .name = \"{}\",\n        .field_count = {},\n        .attr_count = {},\n        .attrs = {{\n",
            st.name,
            st.fields.len(),
            st.attrs.len(),
        ));
        for a in &st.attrs {
            s.push_str(&format!(
                "            {{ .name = \"{}\", .value = \"{}\" }},\n",
                a.name, a.value
            ));
        }
        s.push_str("        },\n        .fields = {\n");
        for f in &st.fields {
            s.push_str(&format!(
                "            {{\n                .name = \"{}\",\n                .type = \"{}\",\n                .kind = {},\n                .is_pointer = {},\n                .array_len = {},\n                .attr_count = {},\n                .attrs = {{\n",
                f.name,
                f.type_name,
                f.kind.c_enum_str(),
                if f.is_pointer { 1 } else { 0 },
                f.array_len,
                f.attrs.len(),
            ));
            for a in &f.attrs {
                s.push_str(&format!(
                    "                    {{ .name = \"{}\", .value = \"{}\" }},\n",
                    a.name, a.value
                ));
            }
            s.push_str("                }\n            },\n");
        }
        s.push_str("        }\n    },\n");
    }
    s.push_str("};\n");
    s.push_str(&format!(
        "static const int _CT_STRUCT_COUNT = {};\n\n",
        structs.len()
    ));

    // Static data for enums
    s.push_str("static const ct_enum _CT_ALL_ENUMS[] = {\n");
    for en in enums {
        s.push_str(&format!(
            "    {{\n        .name = \"{}\",\n        .variant_count = {},\n        .attr_count = {},\n        .attrs = {{\n",
            en.name,
            en.variants.len(),
            en.attrs.len(),
        ));
        for a in &en.attrs {
            s.push_str(&format!(
                "            {{ .name = \"{}\", .value = \"{}\" }},\n",
                a.name, a.value
            ));
        }
        s.push_str("        },\n        .variants = {\n");
        for v in &en.variants {
            s.push_str(&format!(
                "            {{\n                .name = \"{}\",\n                .value = {}LL,\n                .attr_count = {},\n                .attrs = {{\n",
                v.name, v.value, v.attrs.len()
            ));
            for a in &v.attrs {
                s.push_str(&format!(
                    "                    {{ .name = \"{}\", .value = \"{}\" }},\n",
                    a.name, a.value
                ));
            }
            s.push_str("                }\n            },\n");
        }
        s.push_str("        }\n    },\n");
    }
    s.push_str("};\n");
    s.push_str(&format!(
        "static const int _CT_ENUM_COUNT = {};\n\n",
        enums.len()
    ));

    // Functions and Macros
    s.push_str(r#"
// ============================================================================
// Direct Array Access & Query API
// ============================================================================

static inline int struct_count(void) {
    return _CT_STRUCT_COUNT;
}

static inline const ct_struct* get_struct(int index) {
    if (index >= 0 && index < _CT_STRUCT_COUNT) {
        return &_CT_ALL_STRUCTS[index];
    }
    return NULL;
}

static inline const ct_struct* find_struct(const char* name) {
    if (!name) return NULL;
    for (int i = 0; i < _CT_STRUCT_COUNT; i++) {
        if (strcmp(_CT_ALL_STRUCTS[i].name, name) == 0) {
            return &_CT_ALL_STRUCTS[i];
        }
    }
    return NULL;
}

static inline const ct_field* find_field(const ct_struct* s, const char* name) {
    if (!s || !name) return NULL;
    for (int i = 0; i < s->field_count; i++) {
        if (strcmp(s->fields[i].name, name) == 0) {
            return &s->fields[i];
        }
    }
    return NULL;
}

static inline int enum_count(void) {
    return _CT_ENUM_COUNT;
}

static inline const ct_enum* get_enum(int index) {
    if (index >= 0 && index < _CT_ENUM_COUNT) {
        return &_CT_ALL_ENUMS[index];
    }
    return NULL;
}

static inline const ct_enum* find_enum(const char* name) {
    if (!name) return NULL;
    for (int i = 0; i < _CT_ENUM_COUNT; i++) {
        if (strcmp(_CT_ALL_ENUMS[i].name, name) == 0) {
            return &_CT_ALL_ENUMS[i];
        }
    }
    return NULL;
}

static inline const ct_variant* find_variant(const ct_enum* e, const char* name) {
    if (!e || !name) return NULL;
    for (int i = 0; i < e->variant_count; i++) {
        if (strcmp(e->variants[i].name, name) == 0) {
            return &e->variants[i];
        }
    }
    return NULL;
}

// ============================================================================
// Attribute Inspection Helpers
// ============================================================================

static inline bool ct_has_attr(int count, const ct_attr* attrs, const char* name) {
    if (!attrs || !name) return false;
    for (int i = 0; i < count; i++) {
        if (strcmp(attrs[i].name, name) == 0) return true;
    }
    return false;
}

static inline const char* ct_get_attr(int count, const ct_attr* attrs, const char* name) {
    if (!attrs || !name) return NULL;
    for (int i = 0; i < count; i++) {
        if (strcmp(attrs[i].name, name) == 0) return attrs[i].value;
    }
    return NULL;
}

static inline const char* ct_attr_or(int count, const ct_attr* attrs, const char* name, const char* fallback) {
    const char* val = ct_get_attr(count, attrs, name);
    return (val && val[0] != '\0') ? val : fallback;
}

#define struct_has_attr(s, name)          ct_has_attr((s)->attr_count, (s)->attrs, name)
#define struct_get_attr(s, name)          ct_get_attr((s)->attr_count, (s)->attrs, name)
#define struct_attr_or(s, name, fallback) ct_attr_or((s)->attr_count, (s)->attrs, name, fallback)

#define field_has_attr(f, name)           ct_has_attr((f)->attr_count, (f)->attrs, name)
#define field_get_attr(f, name)           ct_get_attr((f)->attr_count, (f)->attrs, name)
#define field_attr_or(f, name, fallback)  ct_attr_or((f)->attr_count, (f)->attrs, name, fallback)

#define enum_has_attr(e, name)            ct_has_attr((e)->attr_count, (e)->attrs, name)
#define enum_get_attr(e, name)            ct_get_attr((e)->attr_count, (e)->attrs, name)

#define variant_has_attr(v, name)         ct_has_attr((v)->attr_count, (v)->attrs, name)
#define variant_get_attr(v, name)         ct_get_attr((v)->attr_count, (v)->attrs, name)

// ============================================================================
// String Utilities (Case Conversion & Formatting for C Codegen)
// ============================================================================

static inline void str_to_lower(char* dest, size_t dest_size, const char* src) {
    if (!dest || dest_size == 0) return;
    size_t i = 0;
    while (src && src[i] && i + 1 < dest_size) {
        dest[i] = (char)tolower((unsigned char)src[i]);
        i++;
    }
    dest[i] = '\0';
}

static inline void str_to_upper(char* dest, size_t dest_size, const char* src) {
    if (!dest || dest_size == 0) return;
    size_t i = 0;
    while (src && src[i] && i + 1 < dest_size) {
        dest[i] = (char)toupper((unsigned char)src[i]);
        i++;
    }
    dest[i] = '\0';
}

static inline const char* str_trim_prefix(const char* str, const char* prefix) {
    if (!str || !prefix) return str;
    size_t pre_len = strlen(prefix);
    if (strncmp(str, prefix, pre_len) == 0) {
        return str + pre_len;
    }
    return str;
}

static inline void str_to_snake_case(char* dest, size_t dest_size, const char* src) {
    if (!dest || dest_size == 0) return;
    size_t si = 0, di = 0;
    while (src && src[si] && di + 2 < dest_size) {
        char c = src[si];
        if (isupper((unsigned char)c) && si > 0) {
            char prev = src[si - 1];
            if (islower((unsigned char)prev) || (si + 1 < strlen(src) && islower((unsigned char)src[si + 1]))) {
                dest[di++] = '_';
            }
        }
        dest[di++] = (char)tolower((unsigned char)c);
        si++;
    }
    dest[di] = '\0';
}

static inline bool str_starts_with(const char* str, const char* prefix) {
    if (!str || !prefix) return false;
    return strncmp(str, prefix, strlen(prefix)) == 0;
}

static inline bool str_ends_with(const char* str, const char* suffix) {
    if (!str || !suffix) return false;
    size_t l1 = strlen(str);
    size_t l2 = strlen(suffix);
    if (l2 > l1) return false;
    return strcmp(str + l1 - l2, suffix) == 0;
}

// ============================================================================
// Code Emission & Template Helpers
// ============================================================================

static inline void emit(const char* str) {
    if (str) fputs(str, stdout);
}

#define emitf(fmt, ...) printf(fmt, ##__VA_ARGS__)

static inline void emit_file(const char* path, const char* content) {
    FILE* f = fopen(path, "w");
    if (f) {
        fputs(content, f);
        fclose(f);
    } else {
        fprintf(stderr, "Comptime error: cannot write to '%s'\n", path);
    }
}

// ============================================================================
// Ergonomic Iteration Macros (Optional Syntactic Sugar)
// ============================================================================

#define for_each_struct(var) \
    for (int _i = 0; _i < struct_count(); _i++) \
        for (const ct_struct* var = get_struct(_i); var; var = NULL)

#define for_each_field(st, var) \
    for (int _f = 0; _f < (st)->field_count; _f++) \
        for (const ct_field* var = &(st)->fields[_f]; var; var = NULL)

#define for_each_enum(var) \
    for (int _i = 0; _i < enum_count(); _i++) \
        for (const ct_enum* var = get_enum(_i); var; var = NULL)

#define for_each_variant(en, var) \
    for (int _v = 0; _v < (en)->variant_count; _v++) \
        for (const ct_variant* var = &(en)->variants[_v]; var; var = NULL)

#endif /* COMPTIME_H */
"#);

    s
}
