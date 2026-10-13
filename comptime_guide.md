# Comptime Metaprogramming & Static Reflection in C
### The Complete Practical Handbook for Engine Architecture and Code Generation

---

## 1. Architecture and Core Principles

In traditional C engine development, three primary approaches are commonly used to eliminate repetitive boilerplate:
1. **X-Macros (`#define X(...)`)** — difficult to read and maintain, incapable of inspecting field types, and lacking conditional branching flexibility.
2. **External Python / Lua / Libclang scripts** — introduce external build-step friction, require extra dependencies, and easily fall out of sync with header files.
3. **Manual copy-pasting** — hundreds of lines of repetitive code for every ECS component, asset loader, or serializer.

**Crupp Comptime** solves this problem at the language level:
* **Step 1 (Metadata Extraction)**: Before compiler invocation, Crupp scans C declarations for `struct`, `typedef struct`, `enum`, fields, and annotations directly from source.
* **Step 2 (Environment Synthesis)**: Crupp synthesizes the reflection database into `<comptime.h>`.
* **Step 3 (In-Memory JIT Execution)**: The embedded Tiny C Compiler (TCC) compiles and executes the `#if comptime` block in a few milliseconds directly in RAM.
* **Step 4 (Code Substitution)**: Anything your comptime C code outputs via `emit()` or `emitf()` (to standard output) replaces the `#if comptime ... #endif` block in the target file.

You write standard, readable C code that generates C code for your program during compilation.

---

## 2. `<comptime.h>` Data Model

All data structures provide direct, unhindered pointer access:

### Attributes (`ct_attr`)
Arbitrary annotations attached to structures, fields, or enums:
```c
typedef struct ct_attr {
    const char* name;   // Attribute name (e.g., "component", "default", "key")
    const char* value;  // Value string (e.g., "64.0f", "width", or "" for flags)
} ct_attr;
```

Supported comment annotations in C:
* `// @component` — flag attribute (`name: "component"`, `value: ""`)
* `// @default("1.0f")` or `// @default(1.0f)` — valued attribute (`name: "default"`, `value: "1.0f"`)
* `// @table("transform")` — custom key attribute
* `// #[id(10)]` or `// #[skip]` — Rust-style syntax is also supported

### Structure Fields (`ct_field`)
```c
typedef struct ct_field {
    const char* name;       // Field identifier ("x", "zoom", "texture")
    const char* type;       // Type string ("f32", "EnAssetId", "char*")
    TypeKind    kind;       // Semantic category (TYPE_FLOAT, TYPE_INT32, TYPE_STRING...)
    int         is_pointer; // 1 if pointer, otherwise 0
    int         array_len;  // Length if fixed-size array (e.g., 32 for name[32]), 0 for scalars
    int         attr_count; // Number of field attributes
    ct_attr     attrs[16];  // Attribute array
} ct_field;
```

Recognized `TypeKind` categories:
* `TYPE_INT8`, `TYPE_INT16`, `TYPE_INT32`, `TYPE_INT64` (recognizes `int8_t`, `short`, `int`, `long`, `i8`, `i16`, `i32`, `i64`)
* `TYPE_UINT8`, `TYPE_UINT16`, `TYPE_UINT32`, `TYPE_UINT64` (recognizes `uint8_t`, `unsigned int`, `size_t`, `u8`, `u16`, `u32`, `u64`, `usize`)
* `TYPE_FLOAT` (`float`, `f32`), `TYPE_DOUBLE` (`double`, `f64`)
* `TYPE_BOOL` (`bool`), `TYPE_CHAR` (`char`), `TYPE_STRING` (`char*`, `const char*`)
* `TYPE_STRUCT`, `TYPE_POINTER`, `TYPE_ARRAY`, `TYPE_UNKNOWN`

### Structures (`ct_struct`)
```c
typedef struct ct_struct {
    const char* name;        // Structure name ("EnCamera2D", "EnTransform2D")
    int         field_count; // Number of fields
    ct_field    fields[64];  // Field array
    int         attr_count;  // Number of attributes
    ct_attr     attrs[32];   // Attribute array
} ct_struct;
```

---

## 3. Direct Data Access & Query API

You can traverse entities using standard, raw `for` loops without syntactic constraints:

```c
int total = struct_count();
for (int i = 0; i < total; i++) {
    const ct_struct* s = get_struct(i);
    printf("Struct: %s (%d fields)\n", s->name, s->field_count);

    for (int j = 0; j < s->field_count; j++) {
        const ct_field* f = &s->fields[j];
        printf("  - %s %s\n", f->type, f->name);
    }
}
```

### Lookup Helpers
* `const ct_struct* find_struct(const char* name)` — look up a struct by identifier.
* `const ct_field* find_field(const ct_struct* s, const char* name)` — look up a field within a struct.
* `const ct_enum* find_enum(const char* name)` — look up an enum by identifier.
* `const ct_variant* find_variant(const ct_enum* e, const char* name)` — look up an enum variant.

### Attribute Inspection Helpers
* `struct_has_attr(s, "tag")` — returns `true` if struct has `@tag`.
* `struct_get_attr(s, "tag")` — returns the value string or `NULL`.
* `struct_attr_or(s, "tag", "fallback")` — returns the value string or a fallback default.
* `field_has_attr(f, "tag")`, `field_get_attr(f, "tag")`, `field_attr_or(f, "tag", "default")`.

### String Utilities for Codegen
* `str_to_lower(dest, size, src)` — convert to lowercase.
* `str_to_upper(dest, size, src)` — convert to uppercase.
* `str_trim_prefix(src, "En")` — strip prefix (`EnCamera2D` $\to$ `Camera2D`).
* `str_to_snake_case(dest, size, src)` — convert CamelCase to snake_case (`Camera2D` $\to$ `camera_2d`).

### Code Emission
* `emit("code")` — emit raw string to generated code.
* `emitf("format %s", val)` — formatted emission (wrapper over `printf`).
* `emit_file("src/generated/ecs.h", content)` — write output to an external header file.

### Shorthand Iteration Macros (Optional)
If you prefer concise iterator syntax over manual index loops, convenient macros are available:
* `for_each_struct(s) { ... }`
* `for_each_field(s, f) { ... }`
* `for_each_enum(e) { ... }`
* `for_each_variant(e, v) { ... }`

---

## 4. Real-World Engine Case Study: TOML Parsers & ECS Scene Spawner

Consider a game engine with 2D components (`EnCamera2D`, `EnTransform2D`, `EnSprite2D`) that must be deserialized from TOML scene files.

### The Problem (Manual Boilerplate)
1. Manual default value initialization for each component (`.scale_x = 1.0f`, `.texture = EN_ASSET_INVALID`, `.zoom = 64.0f`).
2. Repetitive field-by-field deserialization calls (`toml_double_in`, `toml_string_in`, `toml_int_in`).
3. Dedicated asset loading logic (`en_asset_load(engine, ...)`).
4. Custom key renames (field `w` maps to `"width"` in TOML).
5. A massive bundle dispatch loop in `en_scene_load_and_spawn`, manually checking `if (tr_t) { ... }` for every single component.

### The Comptime Solution

#### Step 1: Add Lightweight Annotations to Structs
```c
// components.h
#include "internal.h"
#include <stdbool.h>

// @component @table("camera")
typedef struct {
    f32 x, y;
    // @default(64.0f)
    f32 zoom;
} EnCamera2D;

// @component @table("transform")
typedef struct {
    f32 x, y;
    f32 rotation;
    // @default(1.0f)
    f32 scale_x;
    // @default(1.0f)
    f32 scale_y;
} EnTransform2D;

// @component @table("sprite")
typedef struct {
    // @asset @default(EN_ASSET_INVALID)
    EnAssetId texture;

    // @key("width") @default(1.0f)
    f32 w;
    // @key("height") @default(1.0f)
    f32 h;

    f32 rotation;

    // @default(1.0f)
    f32 r, g, b, a;

    // @default(0)
    i32 z;
} EnSprite2D;
```

#### Step 2: Implement the Comptime Generator
Append this `#if comptime` block to the source file:
```c
#if comptime
#include <comptime.h>

int main(void) {
    int total = struct_count();

    // 1. Generate parse_<name> functions for all structs tagged with @component
    for (int i = 0; i < total; i++) {
        const ct_struct* s = get_struct(i);
        if (!struct_has_attr(s, "component")) continue;

        const char* table = struct_attr_or(s, "table", s->name);

        // Check whether EnEngine* parameter is required (e.g. for asset loading)
        bool has_asset = false;
        for (int j = 0; j < s->field_count; j++) {
            if (field_has_attr(&s->fields[j], "asset")) has_asset = true;
        }

        if (has_asset) {
            emitf("static void parse_%s(EnEngine* engine, toml_table_t* t, %s* out) {\n", table, s->name);
        } else {
            emitf("static void parse_%s(toml_table_t* t, %s* out) {\n", table, s->name);
        }

        // Emit default values struct initializer
        emitf("    *out = (%s){\n", s->name);
        for (int j = 0; j < s->field_count; j++) {
            const ct_field* f = &s->fields[j];
            const char* def = field_get_attr(f, "default");
            if (def && strlen(def) > 0) {
                emitf("        .%s = %s,\n", f->name, def);
            }
        }
        emit("    };\n    toml_datum_t d;\n\n");

        // Emit field deserializers
        for (int j = 0; j < s->field_count; j++) {
            const ct_field* f = &s->fields[j];
            const char* key = field_attr_or(f, "key", f->name);

            if (field_has_attr(f, "asset")) {
                emitf("    d = toml_string_in(t, \"%s\");\n", key);
                emitf("    if (d.ok) {\n");
                emitf("        out->%s = en_asset_load(engine, d.u.s);\n", f->name);
                emitf("        if (out->%s == EN_ASSET_INVALID) en_warn(\"Scene: texture '%%s' could not be loaded\", d.u.s);\n", f->name);
                emitf("        free(d.u.s);\n    }\n");
            } else if (f->kind == TYPE_FLOAT || strcmp(f->type, "f32") == 0) {
                emitf("    d = toml_double_in(t, \"%s\"); if (d.ok) out->%s = (f32)d.u.d;\n", key, f->name);
            } else if (f->kind == TYPE_INT32 || strcmp(f->type, "i32") == 0) {
                emitf("    d = toml_int_in(t, \"%s\"); if (d.ok) out->%s = (i32)d.u.i;\n", key, f->name);
            }
        }
        emit("}\n\n");
    }

    // 2. Generate automated scene bundle populator
    emit("static inline void populate_scene_bundles(EnEngine* engine, toml_table_t* e, EnBundleEntry* bundle, usize* count) {\n");
    for (int i = 0; i < total; i++) {
        const ct_struct* s = get_struct(i);
        if (!struct_has_attr(s, "component")) continue;

        const char* table = struct_attr_or(s, "table", s->name);

        bool has_asset = false;
        for (int j = 0; j < s->field_count; j++) {
            if (field_has_attr(&s->fields[j], "asset")) has_asset = true;
        }

        emitf("    {\n");
        emitf("        toml_table_t* sub = toml_table_in(e, \"%s\");\n", table);
        emitf("        if (sub) {\n");
        emitf("            static %s comp_buf;\n", s->name);
        if (has_asset) {
            emitf("            parse_%s(engine, sub, &comp_buf);\n", table);
        } else {
            emitf("            parse_%s(sub, &comp_buf);\n", table);
        }
        emitf("            bundle[*count].comp_id = engine->comp_%s;\n", table);
        emitf("            bundle[*count].data    = &comp_buf;\n");
        emitf("            (*count)++;\n");
        emitf("        }\n");
        emitf("    }\n");
    }
    emit("}\n");

    return 0;
}
#endif
```

#### Step 3: Streamlined Scene Spawner Function
The manual `if (tr_t) ... if (sp_t) ... if (cam_t)` chain collapses into a single call:

```c
bool en_scene_load_and_spawn(EnEngine* engine, const char* path) {
    // ... file reading and root table parsing ...

    for (int i = 0; i < n; i++) {
        toml_table_t* e = toml_table_at(entities, i);
        if (!e) continue;

        EnBundleEntry bundle[32];
        usize count = 0;

        // ALL DISPATCH BOILERPLATE IS REPLACED HERE:
        populate_scene_bundles(engine, e, bundle, &count);

        if (count == 0) continue;

        en_world_spawn_bundle_raw(engine->world, bundle, count);
        spawned++;
    }

    // ...
    return true;
}
```

### Key Architectural Benefits
* When you add new components (`EnRigidBody2D`, `EnCollider2D`, `EnAudioEmitter`):
  * Define the struct with `@component @table(...)`.
  * The TOML parser is generated **automatically**.
  * Default initializations are handled **automatically**.
  * Bundle registration in `populate_scene_bundles` updates **automatically**.
  * The scene loader function `en_scene_load_and_spawn` **never needs to be edited again**.

---

## 5. Universal Engine Metaprogramming Patterns

### Pattern A: Automated Component Registration
```c
#if comptime
#include <comptime.h>

int main(void) {
    emit("void engine_register_all_components(EnEngine* engine) {\n");
    for (int i = 0; i < struct_count(); i++) {
        const ct_struct* s = get_struct(i);
        if (struct_has_attr(s, "component")) {
            const char* tbl = struct_attr_or(s, "table", s->name);
            emitf("    engine->comp_%s = en_ecs_register(engine->world, \"%s\", sizeof(%s));\n",
                  tbl, s->name, s->name);
        }
    }
    emit("}\n");
    return 0;
}
#endif
```

### Pattern B: ImGui / Editor Field Inspector
Generate runtime reflection with field offsets (`offsetof`) for graphical property inspectors:
```c
#if comptime
#include <comptime.h>

int main(void) {
    emit("void inspect_component(const char* name, void* ptr) {\n");
    for (int i = 0; i < struct_count(); i++) {
        const ct_struct* s = get_struct(i);
        if (!struct_has_attr(s, "component")) continue;

        emitf("    if (strcmp(name, \"%s\") == 0) {\n", s->name);
        emitf("        %s* obj = (%s*)ptr;\n", s->name, s->name);
        for (int j = 0; j < s->field_count; j++) {
            const ct_field* f = &s->fields[j];
            if (f->kind == TYPE_FLOAT) {
                emitf("        igDragFloat(\"%s\", &obj->%s, 0.1f, 0, 0, \"%%.2f\", 0);\n", f->name, f->name);
            } else if (f->kind == TYPE_INT32) {
                emitf("        igDragInt(\"%s\", &obj->%s, 1, 0, 0, \"%%d\", 0);\n", f->name, f->name);
            }
        }
        emit("        return;\n    }\n");
    }
    emit("}\n");
    return 0;
}
#endif
```

### Pattern C: Binary Netcode / Savefile Serializer
```c
#if comptime
#include <comptime.h>

int main(void) {
    for (int i = 0; i < struct_count(); i++) {
        const ct_struct* s = get_struct(i);
        if (!struct_has_attr(s, "serialize")) continue;

        emitf("void serialize_%s(Stream* stream, const %s* val) {\n", s->name, s->name);
        for (int j = 0; j < s->field_count; j++) {
            const ct_field* f = &s->fields[j];
            if (field_has_attr(f, "skip")) continue;
            emitf("    stream_write(stream, &val->%s, sizeof(val->%s));\n", f->name, f->name);
        }
        emit("}\n\n");
    }
    return 0;
}
#endif
```