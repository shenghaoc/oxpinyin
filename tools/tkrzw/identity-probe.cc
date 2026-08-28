// identity-probe.cc — does a libtkrzw build honour tkrzw's pointer-identity
// protocol at the client/library boundary?
//
// tkrzw identifies three things by the address of a symbol rather than by its
// value, and a client binary is expected to agree with libtkrzw about those
// addresses. This probe exercises all three from a *client* translation unit,
// the way any application would. It is the library-API counterpart to
// distro-probe.sh, which only drives tkrzw's own command-line tools.
//
// See docs/findings/tkrzw-distro-compat.md. Build against the library under
// test, e.g.
//
//   g++ -std=c++17 -O2 -I PREFIX/include identity-probe.cc -o identity-probe
//       -L PREFIX/lib -Wl,-rpath,PREFIX/lib -ltkrzw -llzma -llz4 -lzstd -lz -lpthread
//   ./identity-probe /tmp/some-empty-dir
//
// Rows (b)-(e) come back broken on any build linked with
// -Wl,-Bsymbolic-functions, which is every Ubuntu package. Exit status is 0
// when every row is correct and 1 otherwise.

#include <tkrzw_dbm_hash.h>
#include <tkrzw_dbm_tree.h>
#include <tkrzw_key_comparators.h>

#include <cstdio>
#include <string>
#include <string_view>

using namespace tkrzw;

// The TreeDBM comparator type byte, offset 53 of the "TDB" opaque metadata
// block TreeDBM keeps inside its HashDBM container. 1 is the lexical default;
// 255 means "a comparator I could not name", which no later open can resolve.
static int KeyCompByte(const std::string& path) {
  FILE* const file = std::fopen(path.c_str(), "rb");
  if (file == nullptr) return -1;
  unsigned char buf[256];
  const size_t size = std::fread(buf, 1, sizeof buf, file);
  std::fclose(file);
  for (size_t i = 0; i + 56 < size; i++) {
    if (buf[i] == 'T' && buf[i + 1] == 'D' && buf[i + 2] == 'B') {
      return buf[i + 53];
    }
  }
  return -1;
}

// Sentinel bytes are unprintable, so render them rather than write them out.
static std::string Show(std::string_view value) {
  std::string out = "\"";
  for (const unsigned char c : value) {
    if (c >= 0x20 && c < 0x7f) {
      out += static_cast<char>(c);
    } else {
      char esc[5];
      std::snprintf(esc, sizeof esc, "\\x%02x", c);
      out += esc;
    }
  }
  return out + "\"";
}

int main(int argc, char** argv) {
  if (argc < 2) {
    std::fprintf(stderr, "usage: identity-probe DIR\n");
    return 2;
  }
  const std::string dir = argv[1];
  bool healthy = true;

  // (a) TreeDBM with the comparator left at its default. The library picks
  // the comparator itself, so both sides of the pointer comparison are the
  // same copy and this row holds even on a broken build.
  {
    const std::string path = dir + "/a.tkt";
    TreeDBM dbm;
    TreeDBM::TuningParameters params;
    dbm.OpenAdvanced(path, true, File::OPEN_TRUNCATE, params);
    dbm.Set("k", "v");
    dbm.Close();
    TreeDBM reopened;
    const Status status = reopened.Open(path, false);
    reopened.Close();
    std::printf("(a) default comparator (nullptr)        : key_comp_type=%-3d reopen=%s\n",
                KeyCompByte(path),
                status == Status::SUCCESS ? "OK" : ToString(status).c_str());
    healthy &= status == Status::SUCCESS;
  }

  // (b) TreeDBM with the documented built-in comparator, named by the client.
  {
    const std::string path = dir + "/b.tkt";
    TreeDBM dbm;
    TreeDBM::TuningParameters params;
    params.key_comparator = LexicalKeyComparator;  // address taken in THIS binary
    dbm.OpenAdvanced(path, true, File::OPEN_TRUNCATE, params);
    dbm.Set("k", "v");
    dbm.Close();
    TreeDBM reopened;
    const Status status = reopened.Open(path, false);
    reopened.Close();
    std::printf("(b) client passes LexicalKeyComparator  : key_comp_type=%-3d reopen=%s\n",
                KeyCompByte(path),
                status == Status::SUCCESS ? "OK" : ToString(status).c_str());
    healthy &= status == Status::SUCCESS;
  }

  // (c)-(e) The RecordProcessor::NOOP and REMOVE sentinels, which tkrzw's own
  // header says to check with `your_value.data() == NOOP.data()`.
  {
    const std::string path = dir + "/c.tkh";
    HashDBM dbm;
    dbm.Open(path, true, File::OPEN_TRUNCATE);
    dbm.Set("k", "v");

    class NoopProcessor final : public DBM::RecordProcessor {
     public:
      std::string_view ProcessFull(std::string_view, std::string_view) override {
        return NOOP;  // address taken in THIS binary
      }
    } processor;
    dbm.Process("k", &processor, true);

    std::string value;
    dbm.Get("k", &value);
    const bool noop_ok = value == "v";
    std::printf("(c) NOOP-returning processor            : value now %-22s %s\n",
                Show(value).c_str(), noop_ok ? "OK" : "CORRUPTED");
    healthy &= noop_ok;

    dbm.Remove("k");
    std::string leftover;
    const Status got = dbm.Get("k", &leftover);
    const bool remove_ok = got == Status::NOT_FOUND_ERROR;
    std::printf("(d) Remove()                            : %s\n",
                remove_ok ? "OK (record gone)"
                          : ("BROKEN (record still present, value=" +
                             Show(leftover) + ")").c_str());
    healthy &= remove_ok;

    const Status rebuilt = dbm.Rebuild();
    std::printf("(e) Rebuild()                           : %s\n",
                rebuilt == Status::SUCCESS ? "OK" : ToString(rebuilt).c_str());
    healthy &= rebuilt == Status::SUCCESS;
    dbm.Close();
  }

  return healthy ? 0 : 1;
}
