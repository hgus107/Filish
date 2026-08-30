#import <AppKit/AppKit.h>
#import <PDFKit/PDFKit.h>
#import <ImageIO/ImageIO.h>

#include <math.h>
#include <stdio.h>
#include <string.h>

static void quire_set_error(char *buffer, size_t capacity, NSString *message) {
    if (buffer == NULL || capacity == 0) return;
    const char *text = (message ?: @"The PDF could not be processed.").UTF8String;
    snprintf(buffer, capacity, "%s", text ?: "The PDF could not be processed.");
}

static PDFDocument *quire_open_document(const char *path, char *error, size_t error_capacity) {
    if (path == NULL) {
        quire_set_error(error, error_capacity, @"The PDF path is missing.");
        return nil;
    }
    NSString *source = [NSString stringWithUTF8String:path];
    PDFDocument *document = [[PDFDocument alloc] initWithURL:[NSURL fileURLWithPath:source]];
    if (document == nil || document.pageCount == 0) {
        quire_set_error(error, error_capacity, @"The PDF could not be opened.");
        return nil;
    }
    return document;
}

static CGImageRef quire_render_page(PDFPage *page, CGFloat dpi, char *error, size_t error_capacity) {
    NSRect bounds = [page boundsForBox:kPDFDisplayBoxMediaBox];
    CGFloat scale = dpi / 72.0;
    size_t width = (size_t)MAX(1.0, ceil(NSWidth(bounds) * scale));
    size_t height = (size_t)MAX(1.0, ceil(NSHeight(bounds) * scale));
    CGColorSpaceRef color_space = CGColorSpaceCreateDeviceRGB();
    CGContextRef context = CGBitmapContextCreate(NULL, width, height, 8, 0, color_space,
                                                  (CGBitmapInfo)kCGImageAlphaPremultipliedLast);
    CGColorSpaceRelease(color_space);
    if (context == NULL) {
        quire_set_error(error, error_capacity, @"A PDF page could not be rendered.");
        return NULL;
    }
    CGContextSetRGBFillColor(context, 1, 1, 1, 1);
    CGContextFillRect(context, CGRectMake(0, 0, width, height));
    CGContextSaveGState(context);
    CGContextScaleCTM(context, scale, scale);
    CGContextTranslateCTM(context, -NSMinX(bounds), -NSMinY(bounds));
    [page drawWithBox:kPDFDisplayBoxMediaBox toContext:context];
    CGContextRestoreGState(context);
    CGImageRef image = CGBitmapContextCreateImage(context);
    CGContextRelease(context);
    if (image == NULL) quire_set_error(error, error_capacity, @"A PDF page image could not be created.");
    return image;
}

int quire_pdf_page_count(const char *path, char *error, size_t error_capacity) {
    @autoreleasepool {
        PDFDocument *document = quire_open_document(path, error, error_capacity);
        return document == nil ? -1 : (int)document.pageCount;
    }
}

int quire_pdf_extract_text(const char *path, const char *output_path,
                           char *error, size_t error_capacity) {
    @autoreleasepool {
        PDFDocument *document = quire_open_document(path, error, error_capacity);
        if (document == nil) return 0;
        NSMutableString *text = [NSMutableString string];
        for (NSUInteger index = 0; index < document.pageCount; index++) {
            PDFPage *page = [document pageAtIndex:index];
            NSString *page_text = page.string ?: @"";
            [text appendString:page_text];
            if (index + 1 < document.pageCount) [text appendString:@"\n\f\n"];
        }
        NSString *destination = [NSString stringWithUTF8String:output_path];
        NSError *write_error = nil;
        if (![text writeToFile:destination atomically:YES encoding:NSUTF8StringEncoding error:&write_error]) {
            quire_set_error(error, error_capacity, write_error.localizedDescription);
            return 0;
        }
        return 1;
    }
}

int quire_pdf_render_pages(const char *path, const char *output_directory, double dpi,
                           char *error, size_t error_capacity) {
    @autoreleasepool {
        PDFDocument *document = quire_open_document(path, error, error_capacity);
        if (document == nil) return -1;
        NSString *folder = [NSString stringWithUTF8String:output_directory];
        for (NSUInteger index = 0; index < document.pageCount; index++) {
            CGImageRef image = quire_render_page([document pageAtIndex:index], dpi, error, error_capacity);
            if (image == NULL) return -1;
            NSString *name = [NSString stringWithFormat:@"page-%06ld.png", (long)index + 1];
            NSURL *url = [NSURL fileURLWithPath:[folder stringByAppendingPathComponent:name]];
            CGImageDestinationRef destination = CGImageDestinationCreateWithURL((__bridge CFURLRef)url,
                                                                                 CFSTR("public.png"), 1, NULL);
            if (destination == NULL) {
                CGImageRelease(image);
                quire_set_error(error, error_capacity, @"A rendered PDF page could not be created.");
                return -1;
            }
            CGImageDestinationAddImage(destination, image, NULL);
            BOOL written = CGImageDestinationFinalize(destination);
            CFRelease(destination);
            CGImageRelease(image);
            if (!written) {
                quire_set_error(error, error_capacity, @"A rendered PDF page could not be saved.");
                return -1;
            }
        }
        return (int)document.pageCount;
    }
}

int quire_pdf_compress(const char *path, const char *output_path, double dpi, double quality,
                       char *error, size_t error_capacity) {
    @autoreleasepool {
        PDFDocument *document = quire_open_document(path, error, error_capacity);
        if (document == nil) return 0;
        NSString *destination_path = [NSString stringWithUTF8String:output_path];
        NSURL *destination_url = [NSURL fileURLWithPath:destination_path];
        NSDictionary *options = @{};
        if (@available(macOS 13.4, *)) {
            if (dpi <= 100.0) {
                options = @{
                    PDFDocumentSaveImagesAsJPEGOption: @YES,
                    PDFDocumentOptimizeImagesForScreenOption: @YES
                };
            } else if (quality < 0.8) {
                options = @{PDFDocumentSaveImagesAsJPEGOption: @YES};
            }
        }
        if (![document writeToURL:destination_url withOptions:options]) {
            quire_set_error(error, error_capacity, @"The compressed PDF could not be written.");
            return 0;
        }
        return 1;
    }
}
