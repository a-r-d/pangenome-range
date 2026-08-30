#include <charconv>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <string_view>

namespace {

std::string_view field(const std::string& line, std::size_t index) {
  std::size_t start = 0;
  for (std::size_t current = 0; current < index; ++current) {
    start = line.find('\t', start);
    if (start == std::string::npos) {
      throw std::runtime_error("missing GFA field");
    }
    ++start;
  }

  const std::size_t end = line.find('\t', start);
  return std::string_view(line).substr(
      start, end == std::string::npos ? std::string::npos : end - start);
}

std::uint64_t parse_id(std::string_view value) {
  std::uint64_t result = 0;
  const char* begin = value.data();
  const char* end = begin + value.size();
  const auto parsed = std::from_chars(begin, end, result);
  if (parsed.ec != std::errc() || parsed.ptr != end) {
    throw std::runtime_error("non-numeric segment id");
  }
  return result;
}

bool in_range(std::uint64_t id, std::uint64_t low, std::uint64_t high) {
  return id >= low && id <= high;
}

bool walk_is_in_range(
    std::string_view walk,
    std::uint64_t low,
    std::uint64_t high) {
  bool saw_inside = false;
  bool saw_outside = false;
  std::size_t offset = 0;

  while (offset < walk.size()) {
    if (walk[offset] != '>' && walk[offset] != '<') {
      throw std::runtime_error("invalid W-line orientation");
    }
    const std::size_t begin = ++offset;
    while (offset < walk.size() && walk[offset] != '>' && walk[offset] != '<') {
      ++offset;
    }
    if (begin == offset) {
      throw std::runtime_error("empty W-line segment id");
    }

    const bool inside = in_range(parse_id(walk.substr(begin, offset - begin)), low, high);
    saw_inside = saw_inside || inside;
    saw_outside = saw_outside || !inside;
    if (saw_inside && saw_outside) {
      throw std::runtime_error("W-line crosses component boundary");
    }
  }

  return saw_inside;
}

std::uint64_t parse_bound(const char* value, const char* name) {
  const std::string_view text(value);
  const std::uint64_t result = parse_id(text);
  if (result == 0) {
    throw std::runtime_error(std::string(name) + " must be positive");
  }
  return result;
}

}  // namespace

int main(int argc, char** argv) {
  try {
    if (argc != 5) {
      std::cerr << "usage: extract-gfa-component INPUT OUTPUT LOW_NODE HIGH_NODE\n";
      return 2;
    }

    const std::filesystem::path input_path(argv[1]);
    const std::filesystem::path output_path(argv[2]);
    const std::filesystem::path partial_path = output_path.string() + ".part";
    const std::uint64_t low = parse_bound(argv[3], "LOW_NODE");
    const std::uint64_t high = parse_bound(argv[4], "HIGH_NODE");
    if (low > high) {
      throw std::runtime_error("LOW_NODE exceeds HIGH_NODE");
    }

    std::ifstream input(input_path, std::ios::binary);
    if (!input) {
      throw std::runtime_error("cannot open input GFA");
    }
    std::ofstream output(partial_path, std::ios::binary | std::ios::trunc);
    if (!output) {
      throw std::runtime_error("cannot open partial output GFA");
    }

    std::uint64_t headers = 0;
    std::uint64_t segments = 0;
    std::uint64_t links = 0;
    std::uint64_t walks = 0;
    std::uint64_t other = 0;
    std::string line;

    while (std::getline(input, line)) {
      if (line.empty()) {
        continue;
      }

      bool keep = false;
      switch (line.front()) {
        case 'H':
          keep = true;
          ++headers;
          break;
        case 'S': {
          const std::uint64_t id = parse_id(field(line, 1));
          keep = in_range(id, low, high);
          segments += keep;
          break;
        }
        case 'L': {
          const bool from_inside = in_range(parse_id(field(line, 1)), low, high);
          const bool to_inside = in_range(parse_id(field(line, 3)), low, high);
          if (from_inside != to_inside) {
            throw std::runtime_error("L-line crosses component boundary");
          }
          keep = from_inside;
          links += keep;
          break;
        }
        case 'W':
          keep = walk_is_in_range(field(line, 6), low, high);
          walks += keep;
          break;
        default:
          ++other;
          break;
      }

      if (keep) {
        output.write(line.data(), static_cast<std::streamsize>(line.size()));
        output.put('\n');
      }
    }

    if (!input.eof()) {
      throw std::runtime_error("failed while reading input GFA");
    }
    output.close();
    if (!output) {
      throw std::runtime_error("failed while writing output GFA");
    }
    if (headers != 1 || segments == 0 || links == 0 || walks == 0) {
      throw std::runtime_error("incomplete component output");
    }

    std::filesystem::rename(partial_path, output_path);
    std::cout << "headers=" << headers << '\n'
              << "segments=" << segments << '\n'
              << "links=" << links << '\n'
              << "walks=" << walks << '\n'
              << "ignored_records=" << other << '\n';
    return 0;
  } catch (const std::exception& error) {
    std::cerr << "extract-gfa-component: " << error.what() << '\n';
    return 1;
  }
}
