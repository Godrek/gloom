/* The whole of a second executable target. It is compiled by the same build
   but linked into `other-tool`, so it must not appear in `server`. */
static void other_only(void) {}

int main(void) {
    other_only();
    return 0;
}
