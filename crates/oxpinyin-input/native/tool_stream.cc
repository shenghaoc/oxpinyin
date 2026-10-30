// The pin utilities' C++ input streams (074a2219 utils/): std::cin, or a
// std::ifstream opened on a named file, read with std::getline into a
// std::string, as their `while (std::getline(*input, linebuf))` loops do
// (e.g. utils/segment/ngseg.cpp:131-139, :188; utils/storage/
// import_interpolation.cpp:67-69). Compiled only for the `tools` feature.
// An exception leaves through a noexcept function and terminates, as the
// pin's uncaught one does.

#include <cstddef>
#include <fstream>
#include <iostream>
#include <string>

struct oxpinyin_tool_istream {
    std::ifstream file;
    std::istream *input = nullptr;
    std::string line;
};

extern "C" {

// `std::istream * input = &std::cin;` (ngseg.cpp:105).
oxpinyin_tool_istream *oxpinyin_tool_istream_cin() noexcept {
    auto *stream = new oxpinyin_tool_istream;
    stream->input = &std::cin;
    return stream;
}

// `input_file.open(argv[1]);` (ngseg.cpp:131). The caller tests
// `!input_file` with oxpinyin_tool_istream_good before anything else, so
// errno is still the open's for its perror.
oxpinyin_tool_istream *oxpinyin_tool_istream_open(const char *filename) noexcept {
    auto *stream = new oxpinyin_tool_istream;
    stream->file.open(filename);
    stream->input = &stream->file;
    return stream;
}

// `static_cast<bool>(*input)`: what `!input_file` negates.
int oxpinyin_tool_istream_good(const oxpinyin_tool_istream *stream) noexcept {
    return static_cast<bool>(*stream->input);
}

// `static_cast<bool>(std::getline(*input, linebuf))`; the line's bytes stay
// valid until the next call or the close.
int oxpinyin_tool_istream_getline(oxpinyin_tool_istream *stream,
                                  const char **data, std::size_t *size) noexcept {
    bool read = static_cast<bool>(std::getline(*stream->input, stream->line));
    *data = stream->line.data();
    *size = stream->line.size();
    return read;
}

// The stream's end: the ifstream's destructor closes a named file.
void oxpinyin_tool_istream_close(oxpinyin_tool_istream *stream) noexcept {
    delete stream;
}

}
