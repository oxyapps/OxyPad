// Bridges Lexilla to Rust: an IDocument whose text, styles, fold levels and
// line states live on the Rust side and are reached through callbacks, plus
// a small C API to create, configure and run lexers.
//
// Positions are byte offsets in UTF-8 text. No C++ exception crosses into
// Rust: every entry point catches them.

#include <cstdint>
#include <cstring>

#include "ILexer.h"
#include "Lexilla.h"
#include "Scintilla.h"

using Scintilla::IDocument;
using Scintilla::ILexer5;

extern "C" {

// Callbacks into Rust. `ctx` is passed back unchanged.
struct OxyDocument {
    void *ctx;
    intptr_t length;
    void (*get_chars)(void *ctx, char *buffer, intptr_t position, intptr_t length);
    char (*style_at)(void *ctx, intptr_t position);
    intptr_t (*line_from_position)(void *ctx, intptr_t position);
    intptr_t (*line_start)(void *ctx, intptr_t line);
    int (*get_level)(void *ctx, intptr_t line);
    void (*set_level)(void *ctx, intptr_t line, int level);
    int (*get_line_state)(void *ctx, intptr_t line);
    void (*set_line_state)(void *ctx, intptr_t line, int state);
    // `styles` is null when the whole run has `style`.
    void (*set_styles)(void *ctx, intptr_t position, intptr_t length, const char *styles, char style);
};

}

namespace {

class Document final : public IDocument {
public:
    explicit Document(const OxyDocument &doc) : doc(doc) {}

    int SCI_METHOD Version() const override { return Scintilla::dvRelease4; }
    void SCI_METHOD SetErrorStatus(int) override {}
    Sci_Position SCI_METHOD Length() const override { return doc.length; }

    void SCI_METHOD GetCharRange(char *buffer, Sci_Position position, Sci_Position length) const override {
        if (position < 0 || length <= 0 || position >= doc.length) {
            return;
        }
        if (position + length > doc.length) {
            length = doc.length - position;
        }
        doc.get_chars(doc.ctx, buffer, position, length);
    }

    char SCI_METHOD StyleAt(Sci_Position position) const override {
        if (position < 0 || position >= doc.length) {
            return 0;
        }
        return doc.style_at(doc.ctx, position);
    }

    Sci_Position SCI_METHOD LineFromPosition(Sci_Position position) const override {
        if (position < 0) {
            position = 0;
        } else if (position > doc.length) {
            position = doc.length;
        }
        return doc.line_from_position(doc.ctx, position);
    }

    Sci_Position SCI_METHOD LineStart(Sci_Position line) const override {
        if (line < 0) {
            return 0;
        }
        return doc.line_start(doc.ctx, line);
    }

    int SCI_METHOD GetLevel(Sci_Position line) const override {
        return line < 0 ? SC_FOLDLEVELBASE : doc.get_level(doc.ctx, line);
    }

    int SCI_METHOD SetLevel(Sci_Position line, int level) override {
        if (line < 0) {
            return SC_FOLDLEVELBASE;
        }
        const int previous = doc.get_level(doc.ctx, line);
        doc.set_level(doc.ctx, line, level);
        return previous;
    }

    int SCI_METHOD GetLineState(Sci_Position line) const override {
        return line < 0 ? 0 : doc.get_line_state(doc.ctx, line);
    }

    int SCI_METHOD SetLineState(Sci_Position line, int state) override {
        if (line < 0) {
            return 0;
        }
        const int previous = doc.get_line_state(doc.ctx, line);
        doc.set_line_state(doc.ctx, line, state);
        return previous;
    }

    void SCI_METHOD StartStyling(Sci_Position position) override { styling = position; }

    bool SCI_METHOD SetStyleFor(Sci_Position length, char style) override {
        if (length > 0) {
            doc.set_styles(doc.ctx, styling, length, nullptr, style);
            styling += length;
        }
        return true;
    }

    bool SCI_METHOD SetStyles(Sci_Position length, const char *styles) override {
        if (length > 0) {
            doc.set_styles(doc.ctx, styling, length, styles, 0);
            styling += length;
        }
        return true;
    }

    void SCI_METHOD DecorationSetCurrentIndicator(int) override {}
    void SCI_METHOD DecorationFillRange(Sci_Position, int, Sci_Position) override {}
    void SCI_METHOD ChangeLexerState(Sci_Position, Sci_Position) override {}
    int SCI_METHOD CodePage() const override { return SC_CP_UTF8; }
    bool SCI_METHOD IsDBCSLeadByte(char) const override { return false; }
    const char *SCI_METHOD BufferPointer() override { return nullptr; }

    int SCI_METHOD GetLineIndentation(Sci_Position line) override {
        constexpr int tabWidth = 8;
        int indent = 0;
        const Sci_Position end = LineEnd(line);
        for (Sci_Position position = LineStart(line); position < end; position++) {
            const char ch = ByteAt(position);
            if (ch == ' ') {
                indent++;
            } else if (ch == '\t') {
                indent = (indent / tabWidth + 1) * tabWidth;
            } else {
                break;
            }
        }
        return indent;
    }

    Sci_Position SCI_METHOD LineEnd(Sci_Position line) const override {
        const Sci_Position start = LineStart(line);
        Sci_Position end = LineStart(line + 1);
        if (end > start && ByteAt(end - 1) == '\n') {
            end--;
        }
        if (end > start && ByteAt(end - 1) == '\r') {
            end--;
        }
        return end;
    }

    Sci_Position SCI_METHOD GetRelativePosition(Sci_Position start, Sci_Position characterOffset) const override {
        Sci_Position position = start;
        while (characterOffset > 0) {
            if (position >= doc.length) {
                return -1;
            }
            Sci_Position width = 1;
            GetCharacterAndWidth(position, &width);
            position += width;
            characterOffset--;
        }
        while (characterOffset < 0) {
            if (position <= 0) {
                return -1;
            }
            position--;
            while (position > 0 && IsTrailByte(ByteAt(position))) {
                position--;
            }
            characterOffset++;
        }
        return position;
    }

    int SCI_METHOD GetCharacterAndWidth(Sci_Position position, Sci_Position *pWidth) const override {
        unsigned char bytes[4] = {0, 0, 0, 0};
        const Sci_Position available = doc.length - position;
        if (position < 0 || available <= 0) {
            if (pWidth) {
                *pWidth = 1;
            }
            return 0;
        }
        const Sci_Position take = available < 4 ? available : 4;
        doc.get_chars(doc.ctx, reinterpret_cast<char *>(bytes), position, take);

        const unsigned char lead = bytes[0];
        int width = 1;
        int character = lead;
        if (lead >= 0xC2 && lead <= 0xDF) {
            width = 2;
            character = lead & 0x1F;
        } else if (lead >= 0xE0 && lead <= 0xEF) {
            width = 3;
            character = lead & 0x0F;
        } else if (lead >= 0xF0 && lead <= 0xF4) {
            width = 4;
            character = lead & 0x07;
        }
        if (width > 1) {
            bool valid = width <= take;
            for (int i = 1; valid && i < width; i++) {
                if (!IsTrailByte(static_cast<char>(bytes[i]))) {
                    valid = false;
                } else {
                    character = (character << 6) | (bytes[i] & 0x3F);
                }
            }
            if (!valid) {
                // Invalid UTF-8: one byte, as Scintilla treats it.
                width = 1;
                character = lead;
            }
        }
        if (pWidth) {
            *pWidth = width;
        }
        return character;
    }

private:
    static bool IsTrailByte(char ch) { return (static_cast<unsigned char>(ch) & 0xC0) == 0x80; }

    char ByteAt(Sci_Position position) const {
        char ch = 0;
        if (position >= 0 && position < doc.length) {
            doc.get_chars(doc.ctx, &ch, position, 1);
        }
        return ch;
    }

    const OxyDocument &doc;
    Sci_Position styling = 0;
};

} // namespace

extern "C" {

void *oxy_lexer_create(const char *name) {
    try {
        return CreateLexer(name);
    } catch (...) {
        return nullptr;
    }
}

void oxy_lexer_release(void *lexer) {
    try {
        static_cast<ILexer5 *>(lexer)->Release();
    } catch (...) {
    }
}

void oxy_lexer_property_set(void *lexer, const char *key, const char *value) {
    try {
        static_cast<ILexer5 *>(lexer)->PropertySet(key, value);
    } catch (...) {
    }
}

void oxy_lexer_word_list_set(void *lexer, int index, const char *words) {
    try {
        static_cast<ILexer5 *>(lexer)->WordListSet(index, words);
    } catch (...) {
    }
}

// Lexes and then folds `length` bytes from `start`, which must be a line start.
// Returns false when the lexer threw.
bool oxy_lexer_run(void *lexer, const OxyDocument *doc, intptr_t start, intptr_t length, int init_style) {
    try {
        Document document(*doc);
        ILexer5 *lex = static_cast<ILexer5 *>(lexer);
        lex->Lex(start, length, init_style, &document);
        lex->Fold(start, length, init_style, &document);
        return true;
    } catch (...) {
        return false;
    }
}

}
