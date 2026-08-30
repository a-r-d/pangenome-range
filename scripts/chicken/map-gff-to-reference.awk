BEGIN {
  FS = "\t"
  OFS = "\t"
  print "##gff-version 3"
}

FILENAME == ARGV[1] {
  reference_contig[$1] = 1
  next
}

FILENAME == ARGV[2] {
  if ($0 !~ /^#/ && NF >= 9 && $7 != "na") {
    target = $5
    if ($2 == "assembled-molecule" && $3 != "MT") {
      target = "chr" $3
    }
    if (target in reference_contig) {
      refseq_to_graph[$7] = target
    }
  }
  next
}

$0 !~ /^#/ && $3 == "gene" && ($1 in refseq_to_graph) {
  $1 = refseq_to_graph[$1]
  print
}
