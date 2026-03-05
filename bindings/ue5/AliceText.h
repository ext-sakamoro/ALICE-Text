// ALICE-Text UE5 C++ Header
// 20 FFI functions for exception-based text compression
//
// Author: Moroya Sakamoto

#pragma once

#include <cstdint>
#include <cstring>
#include <utility>

// ============================================================================
// C API
// ============================================================================

extern "C" {

// Opaque handles
typedef void* AliceTextHandle;
typedef void* AliceDialogueTableHandle;

// Compressed data result
struct AliceTextCompressedData {
    uint8_t* data;
    uint32_t len;
};

// Compression statistics
struct AliceTextStats {
    uint64_t original_size;
    uint64_t compressed_size;
    double   compression_ratio;
    double   space_savings;
};

// Entropy estimation result
struct AliceTextEntropy {
    double   shannon_entropy;
    double   estimated_ratio;
    uint64_t estimated_size;
    uint64_t original_size;
    double   space_savings;
    double   pattern_coverage;
    uint32_t unique_bytes;
    double   repetition_score;
    uint8_t  is_compressible;
};

// --- Lifecycle ---
AliceTextHandle alice_text_create();
void alice_text_destroy(AliceTextHandle handle);

// --- Compress / Decompress ---
AliceTextCompressedData alice_text_compress(AliceTextHandle handle, const char* text);
char* alice_text_decompress(AliceTextHandle handle, const uint8_t* data, uint32_t len);
AliceTextCompressedData alice_text_compress_tuned(const char* text, uint8_t mode);
char* alice_text_decompress_tuned(const uint8_t* data, uint32_t len);

// --- Stats / Entropy ---
uint8_t alice_text_get_stats(AliceTextHandle handle, AliceTextStats* out);
uint8_t alice_text_estimate_entropy(const char* text, AliceTextEntropy* out);

// --- Dialogue ---
AliceDialogueTableHandle alice_text_dialogue_create();
void    alice_text_dialogue_destroy(AliceDialogueTableHandle handle);
uint8_t alice_text_dialogue_add(AliceDialogueTableHandle handle, uint32_t id, const char* speaker, const char* text);
char*   alice_text_dialogue_get(AliceDialogueTableHandle handle, uint32_t id);
uint32_t alice_text_dialogue_count(AliceDialogueTableHandle handle);
uint32_t alice_text_dialogue_unique_chars(AliceDialogueTableHandle handle);

// --- Memory ---
void alice_text_data_free(uint8_t* data, uint32_t len);
void alice_text_string_free(char* s);

// --- Version ---
const char* alice_text_version();

} // extern "C"

// ============================================================================
// RAII C++ Wrapper
// ============================================================================

namespace AliceText {

/// RAII wrapper for the text compressor
class FCompressor {
public:
    FCompressor()
        : Handle(alice_text_create()) {}

    ~FCompressor() {
        if (Handle) alice_text_destroy(Handle);
    }

    // Move only
    FCompressor(FCompressor&& Other) noexcept : Handle(Other.Handle) { Other.Handle = nullptr; }
    FCompressor& operator=(FCompressor&& Other) noexcept {
        if (this != &Other) {
            if (Handle) alice_text_destroy(Handle);
            Handle = Other.Handle;
            Other.Handle = nullptr;
        }
        return *this;
    }
    FCompressor(const FCompressor&) = delete;
    FCompressor& operator=(const FCompressor&) = delete;

    /// Compress text. Caller owns the returned data and must call FreeData().
    AliceTextCompressedData Compress(const char* Text) {
        return alice_text_compress(Handle, Text);
    }

    /// Decompress data. Caller owns the returned string and must call FreeString().
    char* Decompress(const uint8_t* Data, uint32_t Len) {
        return alice_text_decompress(Handle, Data, Len);
    }

    /// Get last compression statistics.
    bool GetStats(AliceTextStats& Out) const {
        return alice_text_get_stats(Handle, &Out) != 0;
    }

    bool IsValid() const { return Handle != nullptr; }

    static void FreeData(AliceTextCompressedData& D) {
        if (D.data) { alice_text_data_free(D.data, D.len); D.data = nullptr; D.len = 0; }
    }

    static void FreeString(char* S) {
        if (S) alice_text_string_free(S);
    }

private:
    AliceTextHandle Handle = nullptr;
};

/// Static tuned compression utilities
struct FTuned {
    /// Mode: 0=Fast, 1=Balanced, 2=Best
    static AliceTextCompressedData Compress(const char* Text, uint8_t Mode = 1) {
        return alice_text_compress_tuned(Text, Mode);
    }

    static char* Decompress(const uint8_t* Data, uint32_t Len) {
        return alice_text_decompress_tuned(Data, Len);
    }
};

/// Entropy estimation
struct FEntropy {
    static bool Estimate(const char* Text, AliceTextEntropy& Out) {
        return alice_text_estimate_entropy(Text, &Out) != 0;
    }
};

/// RAII wrapper for the dialogue table
class FDialogueTable {
public:
    FDialogueTable()
        : Handle(alice_text_dialogue_create()) {}

    ~FDialogueTable() {
        if (Handle) alice_text_dialogue_destroy(Handle);
    }

    // Move only
    FDialogueTable(FDialogueTable&& Other) noexcept : Handle(Other.Handle) { Other.Handle = nullptr; }
    FDialogueTable& operator=(FDialogueTable&& Other) noexcept {
        if (this != &Other) {
            if (Handle) alice_text_dialogue_destroy(Handle);
            Handle = Other.Handle;
            Other.Handle = nullptr;
        }
        return *this;
    }
    FDialogueTable(const FDialogueTable&) = delete;
    FDialogueTable& operator=(const FDialogueTable&) = delete;

    bool Add(uint32_t Id, const char* Speaker, const char* Text) {
        return alice_text_dialogue_add(Handle, Id, Speaker, Text) != 0;
    }

    /// Get dialogue text. Caller must free with FCompressor::FreeString().
    char* Get(uint32_t Id) const {
        return alice_text_dialogue_get(Handle, Id);
    }

    uint32_t Count() const { return alice_text_dialogue_count(Handle); }
    uint32_t UniqueChars() const { return alice_text_dialogue_unique_chars(Handle); }

    bool IsValid() const { return Handle != nullptr; }

private:
    AliceDialogueTableHandle Handle = nullptr;
};

} // namespace AliceText
