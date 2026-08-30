#include <gbwt/fast_locate.h>
#include <gbwt/gbwt.h>
#include <gbwt/support.h>

#include <sdsl/io.hpp>
#include <sdsl/simple_sds.hpp>

#include <algorithm>
#include <array>
#include <cctype>
#include <cstdint>
#include <cstdlib>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <map>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

namespace {

using gbwt::FastLocate;
using gbwt::GBWT;
using gbwt::Node;
using gbwt::Path;
using gbwt::edge_type;
using gbwt::node_type;
using gbwt::size_type;

constexpr std::array<char, 8> BATCH_INPUT_MAGIC{'P', 'M', 'P', 'O', '0', '0', '0', '1'};
constexpr std::array<char, 8> BATCH_OUTPUT_MAGIC{'P', 'M', 'L', 'O', '0', '0', '0', '1'};
constexpr std::array<char, 8> BRUTE_OUTPUT_MAGIC{'P', 'M', 'B', 'F', '0', '0', '0', '1'};

struct Arguments {
  std::string command;
  std::map<std::string, std::string> values;
};

struct Identity {
  size_type sequence_id = 0;
  size_type path_id = 0;
  bool reverse = false;
  std::string raw_name;
  std::string sample;
  std::string contig;
  size_type haplotype = 0;
  size_type fragment = 0;
  std::string path_sense;
};

[[noreturn]] void fail(const std::string& message) {
  throw std::runtime_error(message);
}

std::string json_escape(std::string_view input) {
  std::ostringstream output;
  for (const unsigned char value : input) {
    switch (value) {
      case '"': output << "\\\""; break;
      case '\\': output << "\\\\"; break;
      case '\b': output << "\\b"; break;
      case '\f': output << "\\f"; break;
      case '\n': output << "\\n"; break;
      case '\r': output << "\\r"; break;
      case '\t': output << "\\t"; break;
      default:
        if (value < 0x20) {
          output << "\\u" << std::hex << std::setw(4) << std::setfill('0')
                 << static_cast<unsigned int>(value) << std::dec;
        } else {
          output << static_cast<char>(value);
        }
    }
  }
  return output.str();
}

std::set<std::string> split_names(const std::string& value) {
  std::set<std::string> result;
  std::istringstream input(value);
  std::string name;
  while (input >> name) { result.insert(name); }
  return result;
}

std::string numeric_name(size_type value) {
  return std::to_string(value);
}

Identity identity_for_sequence(const GBWT& index, size_type sequence_id) {
  if (sequence_id >= index.sequences()) { fail("sequence id is outside the GBWT"); }

  Identity result;
  result.sequence_id = sequence_id;
  result.path_id = index.bidirectional() ? Path::id(sequence_id) : sequence_id;
  result.reverse = index.bidirectional() && Path::is_reverse(sequence_id);
  if (!index.hasMetadata() || !index.metadata.hasPathNames() ||
      !index.metadata.hasSampleNames() || !index.metadata.hasContigNames() ||
      result.path_id >= index.metadata.paths()) {
    fail("complete GBWT path, sample, and contig metadata is required for identity output");
  }

  const gbwt::PathName& path = index.metadata.path(result.path_id);
  result.sample = index.metadata.hasSampleNames() ? index.metadata.sample(path.sample) : numeric_name(path.sample);
  result.contig = index.metadata.hasContigNames() ? index.metadata.contig(path.contig) : numeric_name(path.contig);
  result.haplotype = path.phase;
  result.fragment = path.count;
  result.raw_name = result.sample + "#" + numeric_name(result.haplotype) + "#" + result.contig;
  if (result.fragment != 0) { result.raw_name += "#fragment=" + numeric_name(result.fragment); }

  const std::set<std::string> references = split_names(index.tags.get("reference_samples"));
  if (result.sample == "_gbwt_ref") {
    result.path_sense = "generic";
  } else if (references.count(result.sample) != 0) {
    result.path_sense = "reference";
  } else {
    result.path_sense = "haplotype";
  }
  return result;
}

void write_identity_json(std::ostream& output, const Identity& identity) {
  output << "\"sequence_id\":" << identity.sequence_id
         << ",\"canonical_path_id\":" << identity.path_id
         << ",\"sequence_orientation\":\"" << (identity.reverse ? "reverse" : "forward") << "\""
         << ",\"raw_name\":\"" << json_escape(identity.raw_name) << "\""
         << ",\"sample\":\"" << json_escape(identity.sample) << "\""
         << ",\"contig\":\"" << json_escape(identity.contig) << "\""
         << ",\"haplotype\":" << identity.haplotype
         << ",\"fragment\":" << identity.fragment
         << ",\"path_sense\":\"" << identity.path_sense << "\"";
}

Arguments parse_arguments(int argc, char** argv) {
  if (argc < 2) { return {}; }
  Arguments result;
  result.command = argv[1];
  for (int i = 2; i < argc; ++i) {
    const std::string key = argv[i];
    if (key.rfind("--", 0) != 0) { fail("unexpected positional argument: " + key); }
    if (i + 1 >= argc) { fail("missing value for " + key); }
    result.values[key] = argv[++i];
  }
  return result;
}

const std::string& required(const Arguments& args, const std::string& key) {
  const auto iter = args.values.find(key);
  if (iter == args.values.end()) { fail("missing required argument " + key); }
  return iter->second;
}

size_type parse_u64(const std::string& value, const std::string& label) {
  std::size_t consumed = 0;
  unsigned long long parsed = 0;
  try {
    parsed = std::stoull(value, &consumed, 10);
  } catch (const std::exception&) {
    fail("invalid unsigned integer for " + label + ": " + value);
  }
  if (consumed != value.size()) { fail("invalid unsigned integer for " + label + ": " + value); }
  return static_cast<size_type>(parsed);
}

node_type parse_node(const std::string& value) {
  if (value.empty()) { fail("empty oriented node id"); }
  const char suffix = value.back();
  if (suffix == '+' || suffix == '-') {
    const size_type id = parse_u64(value.substr(0, value.size() - 1), "node id");
    return Node::encode(id, suffix == '-');
  }
  return parse_u64(value, "encoded oriented node id");
}

GBWT load_gbwt(const std::string& filename) {
  GBWT result;
  sdsl::simple_sds::load_from(result, filename);
  return result;
}

FastLocate load_r_index(const GBWT& index, const std::string& filename) {
  FastLocate result;
  if (!sdsl::load_from_file(result, filename)) { fail("cannot load r-index: " + filename); }
  result.setGBWT(index);
  return result;
}

template<class Value>
void write_le(std::ostream& output, Value value) {
  static_assert(std::is_unsigned_v<Value>);
  for (std::size_t i = 0; i < sizeof(Value); ++i) {
    output.put(static_cast<char>((value >> (i * 8U)) & 0xffU));
  }
  if (!output) { fail("binary write failed"); }
}

template<class Value>
Value read_le(std::istream& input) {
  static_assert(std::is_unsigned_v<Value>);
  Value result = 0;
  for (std::size_t i = 0; i < sizeof(Value); ++i) {
    const int byte = input.get();
    if (byte == std::char_traits<char>::eof()) { fail("truncated binary input"); }
    result |= static_cast<Value>(static_cast<unsigned char>(byte)) << (i * 8U);
  }
  return result;
}

void command_metadata(const Arguments& args) {
  const GBWT index = load_gbwt(required(args, "--gbwt"));
  const size_type paths = index.bidirectional() ? index.sequences() / 2 : index.sequences();
  std::cout << "{\"type\":\"metadata\",\"sequences\":" << index.sequences()
            << ",\"paths\":" << paths
            << ",\"bidirectional\":" << (index.bidirectional() ? "true" : "false")
            << ",\"gbwt_nodes\":" << index.size()
            << ",\"reference_samples\":\"" << json_escape(index.tags.get("reference_samples")) << "\"}\n";
  for (size_type path_id = 0; path_id < paths; ++path_id) {
    const size_type sequence_id = index.bidirectional() ? Path::encode(path_id, false) : path_id;
    const Identity identity = identity_for_sequence(index, sequence_id);
    std::cout << "{\"type\":\"path\",";
    write_identity_json(std::cout, identity);
    std::cout << "}\n";
  }
}

void command_node_da(const Arguments& args) {
  const GBWT index = load_gbwt(required(args, "--gbwt"));
  FastLocate locate = load_r_index(index, required(args, "--r-index"));
  const node_type node = parse_node(required(args, "--node"));
  if (!index.contains(node)) { fail("oriented node is not present in the GBWT"); }
  const std::vector<size_type> da = locate.decompressDA(node);
  for (size_type offset = 0; offset < da.size(); ++offset) {
    const Identity identity = identity_for_sequence(index, da[offset]);
    std::cout << "{\"node\":" << node
              << ",\"node_id\":" << Node::id(node)
              << ",\"node_orientation\":\"" << (Node::is_reverse(node) ? "reverse" : "forward") << "\""
              << ",\"record_offset\":" << offset << ',';
    write_identity_json(std::cout, identity);
    std::cout << "}\n";
  }
}

void command_locate(const Arguments& args) {
  const GBWT index = load_gbwt(required(args, "--gbwt"));
  FastLocate locate = load_r_index(index, required(args, "--r-index"));
  const node_type node = parse_node(required(args, "--node"));
  const size_type offset = parse_u64(required(args, "--offset"), "record offset");
  if (!index.contains(node)) { fail("oriented node is not present in the GBWT"); }
  const std::vector<size_type> da = locate.decompressDA(node);
  if (offset >= da.size()) { fail("record offset is outside the node document array"); }
  const Identity identity = identity_for_sequence(index, da[offset]);
  std::cout << "{\"node\":" << node << ",\"node_id\":" << Node::id(node)
            << ",\"node_orientation\":\"" << (Node::is_reverse(node) ? "reverse" : "forward") << "\""
            << ",\"record_offset\":" << offset << ',';
  write_identity_json(std::cout, identity);
  std::cout << "}\n";
}

void command_batch_locate(const Arguments& args) {
  const GBWT index = load_gbwt(required(args, "--gbwt"));
  FastLocate locate = load_r_index(index, required(args, "--r-index"));
  std::ifstream input(required(args, "--input"), std::ios::binary);
  if (!input) { fail("cannot open batch input"); }
  std::array<char, 8> magic{};
  input.read(magic.data(), static_cast<std::streamsize>(magic.size()));
  if (magic != BATCH_INPUT_MAGIC) { fail("invalid batch input magic"); }
  const std::uint64_t count = read_le<std::uint64_t>(input);
  std::ofstream output(required(args, "--output"), std::ios::binary | std::ios::trunc);
  if (!output) { fail("cannot open batch output"); }
  output.write(BATCH_OUTPUT_MAGIC.data(), static_cast<std::streamsize>(BATCH_OUTPUT_MAGIC.size()));
  write_le(output, count);

  node_type cached_node = std::numeric_limits<node_type>::max();
  std::vector<size_type> cached_da;
  for (std::uint64_t i = 0; i < count; ++i) {
    const node_type node = read_le<std::uint64_t>(input);
    const size_type offset = read_le<std::uint64_t>(input);
    if (!index.contains(node)) { fail("batch contains an absent oriented node"); }
    if (node != cached_node) {
      cached_node = node;
      cached_da = locate.decompressDA(node);
    }
    if (offset >= cached_da.size()) { fail("batch contains an invalid record offset"); }
    const Identity identity = identity_for_sequence(index, cached_da[offset]);
    write_le(output, static_cast<std::uint64_t>(node));
    write_le(output, static_cast<std::uint64_t>(offset));
    write_le(output, static_cast<std::uint64_t>(identity.sequence_id));
    write_le(output, static_cast<std::uint64_t>(identity.path_id));
    output.put(static_cast<char>(identity.reverse ? 1 : 0));
    for (int padding = 0; padding < 7; ++padding) { output.put('\0'); }
  }
  if (input.peek() != std::char_traits<char>::eof()) { fail("batch input has trailing bytes"); }
}

void command_brute_force(const Arguments& args) {
  const GBWT index = load_gbwt(required(args, "--gbwt"));
  std::fstream output(required(args, "--output"), std::ios::binary | std::ios::in | std::ios::out | std::ios::trunc);
  if (!output) { fail("cannot open brute-force output"); }
  output.write(BRUTE_OUTPUT_MAGIC.data(), static_cast<std::streamsize>(BRUTE_OUTPUT_MAGIC.size()));
  write_le(output, std::uint64_t{0});
  std::uint64_t count = 0;
  for (size_type sequence_id = 0; sequence_id < index.sequences(); ++sequence_id) {
    edge_type position = index.start(sequence_id);
    size_type sequence_position = 0;
    while (position.first != gbwt::ENDMARKER) {
      write_le(output, static_cast<std::uint64_t>(position.first));
      write_le(output, static_cast<std::uint64_t>(position.second));
      write_le(output, static_cast<std::uint64_t>(sequence_id));
      write_le(output, static_cast<std::uint64_t>(sequence_position));
      ++count;
      ++sequence_position;
      position = index.LF(position);
    }
  }
  output.seekp(static_cast<std::streamoff>(BRUTE_OUTPUT_MAGIC.size()));
  write_le(output, count);
}

void command_verify_brute_force(const Arguments& args) {
  const GBWT index = load_gbwt(required(args, "--gbwt"));
  FastLocate locate = load_r_index(index, required(args, "--r-index"));
  const size_type max_bytes = args.values.count("--max-bytes") != 0
    ? parse_u64(args.values.at("--max-bytes"), "max bytes")
    : (size_type{16} << 30U);

  std::vector<size_type> bases(index.effective() + 1, 0);
  for (node_type node = index.firstNode(); node < index.sigma(); ++node) {
    const size_type comp = index.toComp(node);
    bases[comp + 1] = bases[comp] + index.nodeSize(node);
  }
  const size_type occurrences = bases.back();
  if (occurrences > std::numeric_limits<size_type>::max() / sizeof(size_type) ||
      occurrences * sizeof(size_type) > max_bytes) {
    fail("brute-force DA would exceed --max-bytes");
  }
  std::vector<size_type> expected(occurrences, std::numeric_limits<size_type>::max());
  for (size_type sequence_id = 0; sequence_id < index.sequences(); ++sequence_id) {
    edge_type position = index.start(sequence_id);
    while (position.first != gbwt::ENDMARKER) {
      const size_type comp = index.toComp(position.first);
      const size_type flat_offset = bases[comp] + position.second;
      if (flat_offset >= expected.size()) { fail("brute-force position is outside the flat DA"); }
      if (expected[flat_offset] != std::numeric_limits<size_type>::max()) {
        fail("two source sequences mapped to one GBWT record offset");
      }
      expected[flat_offset] = sequence_id;
      position = index.LF(position);
    }
  }

  size_type compared = 0;
  for (node_type node = index.firstNode(); node < index.sigma(); ++node) {
    if (index.empty(node)) { continue; }
    const std::vector<size_type> actual = locate.decompressDA(node);
    const size_type comp = index.toComp(node);
    if (actual.size() != index.nodeSize(node)) { fail("r-index DA length differs from GBWT record length"); }
    for (size_type offset = 0; offset < actual.size(); ++offset) {
      const size_type wanted = expected[bases[comp] + offset];
      if (wanted == std::numeric_limits<size_type>::max()) { fail("brute-force DA has an unfilled position"); }
      if (actual[offset] != wanted) {
        std::ostringstream message;
        message << "DA mismatch at node " << node << " offset " << offset
                << ": brute force " << wanted << ", r-index " << actual[offset];
        fail(message.str());
      }
      ++compared;
    }
  }
  const size_type paths = index.bidirectional() ? index.sequences() / 2 : index.sequences();
  std::cout << "{\"equal\":true,\"sequences\":" << index.sequences()
            << ",\"paths\":" << paths
            << ",\"occurrences\":" << compared
            << ",\"expected_bytes\":" << expected.size() * sizeof(size_type) << "}\n";
}

void print_help(std::ostream& output) {
  output <<
    "path-membership-oracle commands:\n"
    "  metadata --gbwt FILE\n"
    "  node-da --gbwt FILE --r-index FILE --node ID[+|-]\n"
    "  locate --gbwt FILE --r-index FILE --node ID[+|-] --offset N\n"
    "  batch-locate --gbwt FILE --r-index FILE --input FILE --output FILE\n"
    "  brute-force --gbwt FILE --output FILE\n"
    "  verify-brute-force --gbwt FILE --r-index FILE [--max-bytes N]\n";
}

}  // namespace

int main(int argc, char** argv) {
  try {
    const Arguments args = parse_arguments(argc, argv);
    if (args.command.empty() || args.command == "--help" || args.command == "help") {
      print_help(std::cout);
      return EXIT_SUCCESS;
    }
    if (args.command == "metadata") { command_metadata(args); }
    else if (args.command == "node-da") { command_node_da(args); }
    else if (args.command == "locate") { command_locate(args); }
    else if (args.command == "batch-locate") { command_batch_locate(args); }
    else if (args.command == "brute-force") { command_brute_force(args); }
    else if (args.command == "verify-brute-force") { command_verify_brute_force(args); }
    else { fail("unknown command: " + args.command); }
    return EXIT_SUCCESS;
  } catch (const std::exception& error) {
    std::cerr << "path-membership-oracle: " << error.what() << '\n';
    return EXIT_FAILURE;
  }
}
