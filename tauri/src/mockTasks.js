// Demo task summaries for testing matching before on-device summarizing
// exists. Each set describes the same kind of work in different words, the
// way two people's local AI would, so the matcher has to match by meaning.

export const MOCK_TASKS = {
  leetcode: {
    label: "LeetCode",
    topic: "LeetCode",
    summaries: [
      "Grinding LeetCode medium problems on dynamic programming. Currently stuck on Longest Increasing Subsequence and trying to get from the O(n²) solution down to O(n log n) with binary search.",
      "Practicing coding interview questions on LeetCode, focused on graphs. Working through BFS and DFS problems like Number of Islands and Course Schedule, and reviewing topological sort.",
      "Doing the NeetCode 150 list in Python to prep for software engineering internship interviews. Just finished the sliding window section and starting on two pointers.",
      "Timed LeetCode contest practice: solving array and hashing problems under a 25-minute limit each. Debugging an off-by-one error in a prefix sum solution.",
      "Studying binary trees for technical interviews. Solving LeetCode problems on tree traversal and lowest common ancestor, and writing out recursion by hand before coding it.",
    ],
  },
  apartment: {
    label: "Apartment hunting (SF)",
    topic: "Apartment hunting",
    summaries: [
      "Searching Zillow for a one-bedroom apartment in San Francisco under $3,200 a month. Comparing listings in the Mission and Inner Sunset and checking commute times to SoMa.",
      "Looking for a room in a shared apartment in San Francisco for the summer. Scrolling Facebook housing groups and Craigslist, and messaging people about sublets near Hayes Valley.",
      "Apartment hunting in San Francisco: filtering pet-friendly listings in the Richmond and Noe Valley. Building a spreadsheet of rent, square footage and distance to BART and Muni.",
      "Trying to find an SF apartment before a lease starts next month. Reading about application fees and renter's insurance, and booking viewings for studios in Nob Hill and the Tenderloin.",
      "Researching neighborhoods in San Francisco for a first apartment after graduating. Comparing rent prices in Oakland and the Sunset, and checking reviews of landlords and buildings.",
    ],
  },
};

/// A random summary for `kind`, different from `previous` when possible, so
/// each 5-minute push reads like a fresh summary of the same activity.
export function randomSummary(kind, previous = null) {
  const choices = MOCK_TASKS[kind].summaries.filter((summary) => summary !== previous);
  return choices[Math.floor(Math.random() * choices.length)];
}
