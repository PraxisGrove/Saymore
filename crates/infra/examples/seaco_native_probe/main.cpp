// Experimental native acceptance probe; not linked into Saymore.
// Run only with disposable, non-sensitive fixtures. JSON includes their transcripts.
#include <chrono>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <iterator>
#include <limits>
#include <map>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>
#include <sys/resource.h>

#include <nlohmann/json.hpp>
#include "funasrruntime.h"

using Clock = std::chrono::steady_clock;
using Json = nlohmann::json;

static double milliseconds(Clock::time_point start) {
    return std::chrono::duration<double, std::milli>(Clock::now() - start).count();
}

static std::uint64_t peak_resident_bytes() {
    rusage usage{};
    if (getrusage(RUSAGE_SELF, &usage) != 0) {
        throw std::runtime_error("could not read process memory usage");
    }
#if defined(__APPLE__)
    return static_cast<std::uint64_t>(usage.ru_maxrss);
#else
    return static_cast<std::uint64_t>(usage.ru_maxrss) * 1024;
#endif
}

static std::string read_pcm(const char* path) {
    std::ifstream input(path, std::ios::binary);
    if (!input) throw std::runtime_error("could not open PCM fixture");
    std::string pcm((std::istreambuf_iterator<char>(input)), {});
    if (pcm.empty() || pcm.size() % 2 != 0 ||
        pcm.size() > static_cast<std::size_t>(std::numeric_limits<int>::max())) {
        throw std::runtime_error("fixture must be nonempty mono signed 16-bit PCM");
    }
    return pcm;
}

static std::string join_terms(int argc, char** argv) {
    std::string hints;
    for (int index = 3; index < argc; ++index) {
        const std::string term(argv[index]);
        if (term.empty()) throw std::runtime_error("empty dictionary term");
        for (unsigned char ch : term) {
            if (ch <= 32 || ch == 127) {
                throw std::runtime_error("probe terms must not contain whitespace or controls");
            }
        }
        if (!hints.empty()) hints += ' ';
        hints += term;
    }
    return hints;
}

static Json recognize(FUNASR_HANDLE handle, const std::string& pcm,
                      const std::string& terms, const char* label) {
    std::string vocabulary(terms);
    const auto started = Clock::now();
    const auto embeddings = CompileHotwordEmbedding(handle, vocabulary, ASR_OFFLINE);
    if (embeddings.empty()) throw std::runtime_error("hotword compiler returned no embeddings");
    const double embedding_ms = milliseconds(started);
    FunOfflineReset(handle);
    const auto decoding_started = Clock::now();
    std::unique_ptr<void, decltype(&FunASRFreeResult)> result(
        FunOfflineInferBuffer(handle, pcm.data(), static_cast<int>(pcm.size()),
                              RASRM_CTC_GREEDY_SEARCH, nullptr, embeddings,
                              16000, "pcm", false),
        &FunASRFreeResult);
    if (!result) throw std::runtime_error("native inference returned no result");
    const char* text = FunASRGetResult(result.get(), 0);
    if (!text || !*text) throw std::runtime_error("native inference returned empty text");
    return Json{{"case", label}, {"text", text},
                {"embedding_count", embeddings.size()},
                {"embedding_dimension", embeddings.front().size()},
                {"embedding_ms", embedding_ms},
                {"inference_ms", milliseconds(decoding_started)},
                {"peak_resident_bytes", peak_resident_bytes()}};
}

int main(int argc, char** argv) {
    try {
        if (argc < 3) {
            throw std::runtime_error("usage: seaco_native_probe MODEL_DIR PCM_16K_MONO [TERM ...]");
        }
        const std::string pcm = read_pcm(argv[2]);
        const std::string vocabulary = join_terms(argc, argv);
        std::map<std::string, std::string> configuration{
            {"model-dir", argv[1]}, {"quantize", "false"}};
        const auto started = Clock::now();
        std::unique_ptr<void, decltype(&FunOfflineUninit)> handle(
            FunOfflineInit(configuration, 4, false, 1), &FunOfflineUninit);
        if (!handle) throw std::runtime_error("native model initialization failed");
        Json report{{"model_load_ms", milliseconds(started)},
                    {"audio_seconds", pcm.size() / 32000.0},
                    {"dictionary_term_count", argc - 3},
                    {"runs", Json::array()}};
        const auto first = recognize(handle.get(), pcm, "", "baseline_before");
        report["runs"].push_back(first);
        report["runs"].push_back(recognize(handle.get(), pcm, vocabulary, "with_dictionary"));
        const auto last = recognize(handle.get(), pcm, "", "baseline_after");
        report["runs"].push_back(last);
        const bool isolated = first["text"] == last["text"];
        report["baseline_stable_after_hints"] = isolated;
        std::cout << report.dump(2) << '\n';
        return isolated ? 0 : 1;
    } catch (const std::exception& error) {
        std::cerr << Json{{"error", error.what()}}.dump() << '\n';
        return 1;
    }
}
