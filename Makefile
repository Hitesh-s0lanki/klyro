SRC_DIR=src
flags=-O2 -Wall -std=c2x -I$(SRC_DIR)

SRCS=$(wildcard $(SRC_DIR)/*.c $(SRC_DIR)/*/*.c)
HEADERS=$(wildcard $(SRC_DIR)/*.h $(SRC_DIR)/*/*.h)
OBJS=$(SRCS:.c=.o)

all: klyro

klyro: $(OBJS)
	cc ${flags} $^ -o $@ ${ldflags}

$(SRC_DIR)/%.o: $(SRC_DIR)/%.c $(HEADERS)
	cc ${flags} -c $< -o $@

clean:
	rm -rf $(SRC_DIR)/*.o $(SRC_DIR)/*/*.o klyro
