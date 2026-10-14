/* Platform-only hook adapter. Every shared-memory/atomic operation is language code. */
extern void language_secondary(unsigned long hart);
void __biomiya_secondary_main(unsigned long hart) { language_secondary(hart); }
