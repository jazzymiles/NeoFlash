import flash.display.BitmapData;
import flash.display.Loader;
import flash.events.Event;
import flash.events.IOErrorEvent;
import flash.net.URLRequest;
var queue:Array = ["as1.swf", "as2.swf", "as3.swf", "adobe-fla.swf", "adobe-xfl.swf"];
var active:Loader;
var movie:String;
var remaining:int = 0;
var previousHash:String;
var pixels:BitmapData = new BitmapData(128, 72, true, 0);
function sample(e:Event = null):void {
    if (!active || !active.contentLoaderInfo.bytesLoaded || !active.content) return;
    pixels.fillRect(pixels.rect, 0);
    pixels.draw(active);
    var buf:Vector.<uint> = pixels.getVector(pixels.rect);
    var hash:uint = 0;
    for each (var argb:uint in buf) hash = uint(hash * 31 + argb);
    var hex:String = hash.toString(16);
    if (hex != previousHash) {
        trace("ORACLE_PIXEL " + movie + " " + hex);
        previousHash = hex;
    }
}
function advance(e:Event):void {
    sample();
    if (remaining-- > 0) return;
    if (active) {
        active.contentLoaderInfo.removeEventListener(Event.INIT, sample);
        active.unloadAndStop(true);
        removeChild(active);
        active = null;
    }
    if (!queue.length) {
        removeEventListener(Event.ENTER_FRAME, advance);
        removeEventListener(Event.EXIT_FRAME, sample);
        trace("ORACLE_DONE");
        return;
    }
    movie = queue.shift();
    trace("ORACLE_LOADING " + movie);
    active = new Loader();
    previousHash = null;
    active.contentLoaderInfo.addEventListener(Event.INIT, sample);
    active.contentLoaderInfo.addEventListener(IOErrorEvent.IO_ERROR, function (error:IOErrorEvent):void { trace("ORACLE_ERROR " + error.text); });
    addChild(active);
    active.load(new URLRequest(movie));
    remaining = 24;
}
trace("ORACLE_WRAPPER");
addEventListener(Event.ENTER_FRAME, advance);
addEventListener(Event.EXIT_FRAME, sample);
